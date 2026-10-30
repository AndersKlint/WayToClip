use crate::clipboard::clipboard_entry::ClipboardEntry;
use gtk::{gdk, gdk_pixbuf::PixbufLoader, glib, prelude::*};
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

#[derive(Clone, Hash, PartialEq, Eq)]
struct PreviewKey {
    checksum: String,
    size: i32,
    scale: i32,
}
struct Pixels {
    bytes: Vec<u8>,
    width: i32,
    height: i32,
    rowstride: usize,
    has_alpha: bool,
}

struct DecodedPreview {
    key: PreviewKey,
    pixels: Option<Pixels>,
    next: futures_channel::oneshot::Receiver<DecodedPreview>,
}

pub struct ImagePreview {
    cache: Rc<RefCell<HashMap<PreviewKey, Option<gdk::Texture>>>>,
    sender: Option<mpsc::Sender<(PreviewKey, Vec<u8>)>>,
    is_cancelled: Arc<AtomicBool>,
    completion: glib::JoinHandle<()>,
}

impl ImagePreview {
    pub fn new(on_loaded: impl Fn() + 'static) -> Self {
        let cache = Rc::new(RefCell::new(HashMap::new()));
        let (sender, requests) = mpsc::channel::<(PreviewKey, Vec<u8>)>();
        let (mut results, mut receiver) = futures_channel::oneshot::channel::<DecodedPreview>();
        let is_cancelled = Arc::new(AtomicBool::new(false));
        let cancellation = is_cancelled.clone();
        std::thread::spawn(move || {
            while let Ok((key, bytes)) = requests.recv() {
                if cancellation.load(Ordering::Relaxed) {
                    break;
                }
                let pixels = decode(&bytes, &key, &cancellation);
                if cancellation.load(Ordering::Relaxed) {
                    break;
                }
                let (next_sender, next) = futures_channel::oneshot::channel();
                if results.send(DecodedPreview { key, pixels, next }).is_err() {
                    break;
                }
                results = next_sender;
            }
        });
        let weak_cache = Rc::downgrade(&cache);
        let completion = glib::MainContext::default().spawn_local(async move {
            while let Ok(DecodedPreview { key, pixels, next }) = receiver.await {
                receiver = next;
                let Some(cache) = weak_cache.upgrade() else {
                    break;
                };
                let texture = pixels.map(|pixels: Pixels| {
                    let format = if pixels.has_alpha {
                        gdk::MemoryFormat::R8g8b8a8
                    } else {
                        gdk::MemoryFormat::R8g8b8
                    };
                    gdk::MemoryTexture::new(
                        pixels.width,
                        pixels.height,
                        format,
                        &glib::Bytes::from_owned(pixels.bytes),
                        pixels.rowstride,
                    )
                    .upcast()
                });
                cache.borrow_mut().insert(key, texture);
                on_loaded();
            }
        });
        Self {
            cache,
            sender: Some(sender),
            is_cancelled,
            completion,
        }
    }

    pub fn widget(
        &self,
        entry: &ClipboardEntry,
        checksum: &str,
        size: i32,
        scale: i32,
    ) -> gtk::Widget {
        let size = size.clamp(32, 512);
        let scale = scale.max(1);
        let key = PreviewKey {
            checksum: checksum.to_owned(),
            size,
            scale,
        };
        let texture = {
            let mut cache = self.cache.borrow_mut();
            if !cache.contains_key(&key) {
                cache.insert(key.clone(), None);
                if let Some(sender) = &self.sender {
                    let _ = sender.send((key.clone(), entry.as_bytes().to_vec()));
                }
            }
            cache.get(&key).cloned().flatten()
        };
        let frame = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        frame.add_css_class("waytoclip-item-image");
        frame.set_hexpand(true);
        if let Some(texture) = texture {
            let width = (texture.width() + scale - 1) / scale;
            let height = (texture.height() + scale - 1) / scale;
            let picture = gtk::Picture::for_paintable(&texture);
            picture.set_can_shrink(true);
            picture.set_content_fit(gtk::ContentFit::Contain);
            picture.set_halign(gtk::Align::Start);
            picture.set_size_request(width.min(320), height.min(size));
            frame.append(&picture);
        } else {
            let icon = gtk::Image::from_icon_name("image-x-generic-symbolic");
            icon.set_pixel_size(32);
            icon.set_halign(gtk::Align::Start);
            frame.set_height_request(size);
            frame.append(&icon);
        }
        frame.upcast()
    }
}

impl Drop for ImagePreview {
    fn drop(&mut self) {
        self.is_cancelled.store(true, Ordering::Relaxed);
        self.sender.take();
        self.completion.abort();
    }
}

fn decode(bytes: &[u8], key: &PreviewKey, is_cancelled: &AtomicBool) -> Option<Pixels> {
    let loader = PixbufLoader::new();
    let max_height = key.size.saturating_mul(key.scale);
    let max_width = 320_i32.saturating_mul(key.scale);
    let scale = key.scale;
    loader.connect_size_prepared(move |loader, width, height| {
        if width <= 0 || height <= 0 {
            return;
        }
        let ratio = (max_height as f64 / height as f64)
            .min(max_width as f64 / width as f64)
            .min(scale as f64);
        loader.set_size(
            (width as f64 * ratio).round().max(1.0) as i32,
            (height as f64 * ratio).round().max(1.0) as i32,
        );
    });
    let mut is_valid = true;
    for chunk in bytes.chunks(64 * 1024) {
        if is_cancelled.load(Ordering::Relaxed) || loader.write(chunk).is_err() {
            is_valid = false;
            break;
        }
    }
    let is_closed = loader.close().is_ok();
    if !is_valid || !is_closed {
        return None;
    }
    let pixbuf = loader.animation()?.static_image()?;
    if pixbuf.bits_per_sample() != 8 || !matches!(pixbuf.n_channels(), 3 | 4) {
        return None;
    }
    Some(Pixels {
        bytes: pixbuf.read_pixel_bytes().as_ref().to_vec(),
        width: pixbuf.width(),
        height: pixbuf.height(),
        rowstride: pixbuf.rowstride() as usize,
        has_alpha: pixbuf.has_alpha(),
    })
}
