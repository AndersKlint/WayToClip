use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    path::PathBuf,
    rc::Rc,
};

use gtk::gio::prelude::*;
use gtk::{gio, glib};
use serde_json::Value;

use crate::{clipboard::clipboard_entry::ClipboardEntry, settings::SettingsSnapshot};

const CACHE_OWNER: &str = "org.waytoclip.WayToClip";
const REGISTRY_FILE: &str = "registry.txt";

struct ImageOperation {
    entry: ClipboardEntry,
    should_delete: bool,
    sender: futures_channel::oneshot::Sender<Result<(), glib::Error>>,
}

pub struct Registry {
    directory: PathBuf,
    registry_path: PathBuf,
    backup_path: PathBuf,
    pending: RefCell<Option<Vec<ClipboardEntry>>>,
    image_operations: RefCell<VecDeque<ImageOperation>>,
    in_flight: Cell<bool>,
    flush_waiters: RefCell<Vec<futures_channel::oneshot::Sender<Result<(), glib::Error>>>>,
    last_error: RefCell<Option<String>>,
}

impl Registry {
    pub fn new() -> Rc<Self> {
        let directory = glib::user_cache_dir().join(CACHE_OWNER);
        let registry_path = directory.join(REGISTRY_FILE);
        let mut backup_name = registry_path.clone().into_os_string();
        backup_name.push("~");
        Rc::new(Self {
            directory,
            registry_path,
            backup_path: PathBuf::from(backup_name),
            pending: RefCell::new(None),
            image_operations: RefCell::new(VecDeque::new()),
            in_flight: Cell::new(false),
            flush_waiters: RefCell::new(Vec::new()),
            last_error: RefCell::new(None),
        })
    }

    pub async fn read(
        &self,
        snapshot: &SettingsSnapshot,
    ) -> Result<Vec<ClipboardEntry>, glib::Error> {
        let file = gio::File::for_path(&self.registry_path);
        let info = match file
            .query_info_future(
                "standard::size",
                gio::FileQueryInfoFlags::NONE,
                glib::Priority::DEFAULT,
            )
            .await
        {
            Ok(info) => info,
            Err(error) if error.matches(gio::IOErrorEnum::NotFound) => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };

        let size_limit = snapshot.cache_file_size_mb as i64 * 1024 * 1024;
        if info.size() >= size_limit {
            let backup = gio::File::for_path(&self.backup_path);
            let (move_future, _progress) = file.move_future(
                &backup,
                gio::FileCopyFlags::OVERWRITE,
                glib::Priority::DEFAULT,
            );
            let _ = move_future.await;
            return Ok(Vec::new());
        }

        let (contents, _etag) = match file.load_contents_future().await {
            Ok(contents) => contents,
            Err(error) if error.matches(gio::IOErrorEnum::NotFound) => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };

        let parsed: Value = match serde_json::from_slice(contents.as_ref()) {
            Ok(parsed) => parsed,
            Err(_) => return Ok(Vec::new()),
        };
        let Some(items) = parsed.as_array() else {
            return Ok(Vec::new());
        };

        let mut entries = Vec::new();
        for item in items {
            if let Some(entry) = self.read_entry(item).await {
                entries.push(entry);
            }
        }
        trim_non_favorites(&mut entries, snapshot.max_registry_length);
        Ok(entries)
    }

    pub fn write(self: &Rc<Self>, entries: Vec<ClipboardEntry>) {
        *self.pending.borrow_mut() = Some(entries);
        self.start_write();
    }

    pub async fn flush(&self) -> Result<(), glib::Error> {
        if !self.in_flight.get() && self.pending.borrow().is_none() {
            return self.take_error();
        }
        let (sender, receiver) = futures_channel::oneshot::channel();
        self.flush_waiters.borrow_mut().push(sender);
        receiver.await.unwrap_or_else(|_| {
            Err(glib::Error::new(
                gio::IOErrorEnum::Failed,
                "registry flush waiter was dropped",
            ))
        })
    }

    pub async fn clear_cache_folder(&self) -> Result<(), glib::Error> {
        self.flush().await?;
        let folder = gio::File::for_path(&self.directory);
        let enumerator = match folder
            .enumerate_children_future(
                "standard::name",
                gio::FileQueryInfoFlags::NONE,
                glib::Priority::DEFAULT,
            )
            .await
        {
            Ok(enumerator) => enumerator,
            Err(error) if error.matches(gio::IOErrorEnum::NotFound) => return Ok(()),
            Err(error) => return Err(error),
        };

        loop {
            let infos = match enumerator
                .next_files_future(32, glib::Priority::DEFAULT)
                .await
            {
                Ok(infos) => infos,
                Err(error) => {
                    let _ = enumerator.close_future(glib::Priority::DEFAULT).await;
                    return Err(error);
                }
            };
            if infos.is_empty() {
                break;
            }
            for info in infos {
                let child = enumerator.child(&info);
                let _ = child.delete_future(glib::Priority::DEFAULT).await;
            }
        }
        let _ = enumerator.close_future(glib::Priority::DEFAULT).await;
        Ok(())
    }

    pub fn write_entry_file(
        self: &Rc<Self>,
        entry: &ClipboardEntry,
    ) -> impl std::future::Future<Output = Result<(), glib::Error>> + use<> {
        self.queue_image_operation(entry, false)
    }

    pub fn delete_entry_file(
        self: &Rc<Self>,
        entry: &ClipboardEntry,
    ) -> impl std::future::Future<Output = Result<(), glib::Error>> + use<> {
        self.queue_image_operation(entry, true)
    }

    fn queue_image_operation(
        self: &Rc<Self>,
        entry: &ClipboardEntry,
        should_delete: bool,
    ) -> impl std::future::Future<Output = Result<(), glib::Error>> + use<> {
        let (sender, receiver) = futures_channel::oneshot::channel();
        self.image_operations
            .borrow_mut()
            .push_back(ImageOperation {
                entry: entry.clone(),
                should_delete,
                sender,
            });
        self.start_write();
        async move {
            receiver.await.unwrap_or_else(|_| {
                Err(glib::Error::new(
                    gio::IOErrorEnum::Failed,
                    "image operation worker stopped",
                ))
            })
        }
    }

    async fn remove_image_file(&self, entry: &ClipboardEntry) -> Result<(), glib::Error> {
        let file = gio::File::for_path(self.entry_filename(entry));
        match file.delete_future(glib::Priority::DEFAULT).await {
            Ok(()) => Ok(()),
            Err(error) if error.matches(gio::IOErrorEnum::NotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }

    async fn read_entry(&self, item: &Value) -> Option<ClipboardEntry> {
        let object = item.as_object()?;
        let mimetype = object
            .get("mimetype")
            .and_then(Value::as_str)
            .unwrap_or(ClipboardEntry::CANONICAL_PLAIN_TEXT_MIMETYPE);
        let is_favorite = object
            .get("favorite")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if ClipboardEntry::is_text_mimetype(mimetype) {
            let text = object.get("contents").and_then(Value::as_str).unwrap_or("");
            return Some(ClipboardEntry::new(
                mimetype,
                text.as_bytes().to_vec(),
                is_favorite,
            ));
        }

        let filename = object.get("contents").and_then(Value::as_str)?;
        let file = gio::File::for_path(filename);
        let (bytes, _etag) = file.load_contents_future().await.ok()?;
        Some(ClipboardEntry::new(
            mimetype,
            bytes.as_ref().to_vec(),
            is_favorite,
        ))
    }

    fn start_write(self: &Rc<Self>) {
        if self.in_flight.get()
            || (self.pending.borrow().is_none() && self.image_operations.borrow().is_empty())
        {
            return;
        }
        self.in_flight.set(true);
        let this = Rc::clone(self);
        let _ = glib::spawn_future_local(this.process_queue());
    }

    async fn process_queue(self: Rc<Self>) {
        loop {
            let operation = self.image_operations.borrow_mut().pop_front();
            if let Some(operation) = operation {
                let result = if operation.should_delete {
                    self.remove_image_file(&operation.entry).await
                } else {
                    self.save_image_file(&operation.entry).await
                };
                if let Err(error) = &result {
                    *self.last_error.borrow_mut() = Some(error.to_string());
                }
                let _ = operation.sender.send(result);
                continue;
            }
            let entries = self.pending.borrow_mut().take();
            let Some(entries) = entries else {
                break;
            };
            if let Err(error) = self.write_entries(entries).await {
                *self.last_error.borrow_mut() = Some(error.to_string());
            }
        }
        self.in_flight.set(false);
        if self.pending.borrow().is_some() {
            self.start_write();
        } else {
            self.resolve_flushes();
        }
    }

    async fn write_entries(&self, entries: Vec<ClipboardEntry>) -> Result<(), glib::Error> {
        make_directory_all(self.directory.clone()).await?;

        let mut items = Vec::new();
        for entry in &entries {
            if entry.is_text() {
                items.push(registry_item(
                    entry.is_favorite(),
                    &entry.mimetype,
                    &entry.get_string_value(),
                ));
            } else if entry.is_image() {
                let filename = self.entry_filename(entry);
                if let Err(error) = self.save_image_file(entry).await {
                    *self.last_error.borrow_mut() = Some(error.to_string());
                    continue;
                }
                items.push(registry_item(
                    entry.is_favorite(),
                    &entry.mimetype,
                    &filename.to_string_lossy(),
                ));
            }
        }

        let json = format!("[{}]", items.join(","));
        let tmp_file = gio::File::for_path(self.temporary_path());
        let destination = gio::File::for_path(&self.registry_path);
        let bytes = glib::Bytes::from_owned(json.into_bytes());
        tmp_file
            .replace_contents_bytes_future(
                &bytes,
                None,
                false,
                gio::FileCreateFlags::REPLACE_DESTINATION,
            )
            .await?;
        let (move_future, _progress) = tmp_file.move_future(
            &destination,
            gio::FileCopyFlags::OVERWRITE,
            glib::Priority::DEFAULT,
        );
        move_future.await
    }

    fn entry_filename(&self, entry: &ClipboardEntry) -> PathBuf {
        self.directory.join(format!(
            "{}{}",
            entry.checksum(),
            extension_for(&entry.mimetype)
        ))
    }

    async fn save_image_file(&self, entry: &ClipboardEntry) -> Result<(), glib::Error> {
        let file = gio::File::for_path(self.entry_filename(entry));
        match file
            .query_info_future(
                "standard::type",
                gio::FileQueryInfoFlags::NONE,
                glib::Priority::DEFAULT,
            )
            .await
        {
            Ok(_) => return Ok(()),
            Err(error) if error.matches(gio::IOErrorEnum::NotFound) => {}
            Err(error) => return Err(error),
        }
        make_directory_all(self.directory.clone()).await?;
        let bytes = glib::Bytes::from_owned(entry.bytes.clone());
        file.replace_contents_bytes_future(
            &bytes,
            None,
            false,
            gio::FileCreateFlags::REPLACE_DESTINATION,
        )
        .await?;
        Ok(())
    }

    fn temporary_path(&self) -> PathBuf {
        let mut name = self.registry_path.clone().into_os_string();
        name.push(".tmp");
        PathBuf::from(name)
    }

    fn take_error(&self) -> Result<(), glib::Error> {
        match self.last_error.borrow_mut().take() {
            Some(message) => Err(glib::Error::new(gio::IOErrorEnum::Failed, &message)),
            None => Ok(()),
        }
    }

    fn resolve_flushes(&self) {
        let mut waiters = self.flush_waiters.borrow_mut();
        if waiters.is_empty() {
            return;
        }
        let error = self.last_error.borrow_mut().take();
        for waiter in waiters.drain(..) {
            let result = match &error {
                Some(message) => Err(glib::Error::new(gio::IOErrorEnum::Failed, message)),
                None => Ok(()),
            };
            let _ = waiter.send(result);
        }
    }
}

fn trim_non_favorites(entries: &mut Vec<ClipboardEntry>, max_size: usize) {
    let mut non_favorites = entries.iter().filter(|entry| !entry.is_favorite()).count();
    while non_favorites > max_size {
        let Some(index) = entries.iter().position(|entry| !entry.is_favorite()) else {
            break;
        };
        entries.remove(index);
        non_favorites -= 1;
    }
}

fn registry_item(is_favorite: bool, mimetype: &str, contents: &str) -> String {
    format!(
        "{{\"favorite\":{is_favorite},\"mimetype\":{},\"contents\":{}}}",
        json_string(mimetype),
        json_string(contents)
    )
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("JSON string serialization cannot fail")
}

fn extension_for(mimetype: &str) -> &'static str {
    match mimetype {
        "image/png" => ".png",
        "image/jpeg" | "image/jpg" => ".jpg",
        "image/gif" => ".gif",
        "image/webp" => ".webp",
        "image/svg+xml" => ".svg",
        _ => ".bin",
    }
}

async fn make_directory_all(path: PathBuf) -> Result<(), glib::Error> {
    let (sender, receiver) = futures_channel::oneshot::channel();
    std::thread::spawn(move || {
        let result = std::fs::create_dir_all(&path).map_err(|error| {
            glib::Error::new(
                gio::IOErrorEnum::Failed,
                &format!("cannot create {}: {error}", path.display()),
            )
        });
        let _ = sender.send(result);
    });
    receiver.await.unwrap_or_else(|_| {
        Err(glib::Error::new(
            gio::IOErrorEnum::Failed,
            "cache directory worker stopped",
        ))
    })
}
