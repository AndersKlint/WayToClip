mod worker;

use std::{
    io::Write,
    os::unix::net::UnixStream,
    sync::{Arc, Mutex, mpsc},
};

use futures_channel::oneshot;
use gtk::{gio, glib};

use worker::Command;

#[derive(Clone, Default)]
pub struct FocusedApp {
    pub app_id: String,
    pub wm_class: String,
}

pub type FocusedAppHandle = Arc<Mutex<FocusedApp>>;

pub fn focused_app_handle() -> FocusedAppHandle {
    Arc::new(Mutex::new(FocusedApp::default()))
}

#[derive(Clone)]
pub struct OfferSnapshot {
    pub serial: u64,
    pub mimetypes: Vec<String>,
    pub app_id: String,
    pub wm_class: String,
    pub is_own: bool,
}

pub struct KdeWayland {
    commands: mpsc::SyncSender<Command>,
    wakeup: UnixStream,
}

impl KdeWayland {
    pub async fn connect(focused: FocusedAppHandle) -> Result<Self, glib::Error> {
        let (wakeup, receiver) = UnixStream::pair().map_err(io_error)?;
        wakeup.set_nonblocking(true).map_err(io_error)?;
        receiver.set_nonblocking(true).map_err(io_error)?;
        let (commands, incoming) = mpsc::sync_channel(16);
        let backend = Self { commands, wakeup };
        let (ready, result) = oneshot::channel();
        std::thread::Builder::new()
            .name("waytoclip-wayland".into())
            .spawn(move || worker::run(incoming, receiver, ready, focused))
            .map_err(io_error)?;
        result.await.map_err(|_| stopped())??;
        Ok(backend)
    }

    pub async fn watch(&self, after_serial: u64) -> Result<OfferSnapshot, glib::Error> {
        let (reply, result) = oneshot::channel();
        self.send(Command::Watch(after_serial, reply))?;
        result.await.map_err(|_| stopped())?
    }

    pub async fn read(&self, serial: u64, mimetype: &str) -> Result<Vec<u8>, glib::Error> {
        if mimetype.is_empty() || mimetype.len() > worker::MAX_MIME_BYTES || mimetype.contains('\0')
        {
            return Err(error(
                gio::IOErrorEnum::InvalidData,
                "Invalid clipboard MIME type",
            ));
        }
        let (reply, result) = oneshot::channel();
        self.send(Command::Read(serial, mimetype.to_owned(), reply))?;
        result.await.map_err(|_| stopped())?
    }

    pub async fn write(&self, mimetype: &str, bytes: Vec<u8>) -> Result<(), glib::Error> {
        if mimetype.is_empty() || mimetype.len() > worker::MAX_MIME_BYTES || mimetype.contains('\0')
        {
            return Err(error(
                gio::IOErrorEnum::InvalidData,
                "Invalid clipboard MIME type",
            ));
        }
        if bytes.len() > worker::MAX_BYTES {
            return Err(error(
                gio::IOErrorEnum::NoSpace,
                "Clipboard payload exceeds 16 MiB",
            ));
        }
        let (reply, result) = oneshot::channel();
        self.send(Command::Write(mimetype.to_owned(), bytes, reply))?;
        result.await.map_err(|_| stopped())?
    }

    pub async fn paste(&self, is_terminal: bool) -> Result<(), glib::Error> {
        let (reply, result) = oneshot::channel();
        self.send(Command::Paste(is_terminal, reply))?;
        result.await.map_err(|_| stopped())?
    }

    fn send(&self, command: Command) -> Result<(), glib::Error> {
        self.commands
            .try_send(command)
            .map_err(|cause| match cause {
                mpsc::TrySendError::Full(_) => {
                    error(gio::IOErrorEnum::Busy, "KDE Wayland command queue is full")
                }
                mpsc::TrySendError::Disconnected(_) => stopped(),
            })?;
        match (&self.wakeup).write(&[1]) {
            Ok(_) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(()),
            Err(error) => Err(io_error(error)),
        }
    }
}

impl Drop for KdeWayland {
    fn drop(&mut self) {
        let _ = self.send(Command::Shutdown);
    }
}

fn error(code: gio::IOErrorEnum, message: &str) -> glib::Error {
    glib::Error::new(code, message)
}

fn io_error(cause: impl std::fmt::Display) -> glib::Error {
    error(gio::IOErrorEnum::Failed, &cause.to_string())
}

fn stopped() -> glib::Error {
    error(gio::IOErrorEnum::Closed, "KDE Wayland worker stopped")
}
