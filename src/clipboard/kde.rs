mod worker;

use std::{io::Write, os::unix::net::UnixStream, sync::mpsc};

use futures_channel::oneshot;
use gtk::{gio, glib};

use worker::Command;

pub struct KdeWayland {
    commands: mpsc::SyncSender<Command>,
    wakeup: UnixStream,
}

impl KdeWayland {
    pub async fn connect() -> Result<Self, glib::Error> {
        let (wakeup, receiver) = UnixStream::pair().map_err(io_error)?;
        wakeup.set_nonblocking(true).map_err(io_error)?;
        receiver.set_nonblocking(true).map_err(io_error)?;
        let (commands, incoming) = mpsc::sync_channel(8);
        let backend = Self { commands, wakeup };
        let (ready, result) = oneshot::channel();
        std::thread::Builder::new()
            .name("waytoclip-wayland".into())
            .spawn(move || worker::run(incoming, receiver, ready))
            .map_err(io_error)?;
        result.await.map_err(|_| stopped())??;
        Ok(backend)
    }

    pub async fn read_text(&self) -> Result<String, glib::Error> {
        let (reply, result) = oneshot::channel();
        self.send(Command::Read(reply))?;
        result.await.map_err(|_| stopped())?
    }

    pub async fn set_text(&self, text: &str) -> Result<(), glib::Error> {
        if text.len() > worker::MAX_TEXT_BYTES {
            return Err(error(
                gio::IOErrorEnum::NoSpace,
                "Clipboard text exceeds 16 MiB",
            ));
        }
        let (reply, result) = oneshot::channel();
        self.send(Command::Set(text.to_owned(), reply))?;
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
