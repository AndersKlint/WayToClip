mod gnome;
mod kde;

use gtk::{gio, glib, prelude::*};

pub struct PopupTarget {
    pub token: u32,
    pub app_id: String,
    pub is_terminal: bool,
}

pub enum Bridge {
    Gnome(gnome::Bridge),
    Kde(kde::Bridge),
}

impl Bridge {
    pub async fn connect(application: &gtk::Application) -> Result<Self, glib::Error> {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        for name in desktop.split(':') {
            if name.eq_ignore_ascii_case("KDE") {
                if !std::env::var("XDG_SESSION_TYPE")
                    .is_ok_and(|s| s.eq_ignore_ascii_case("wayland"))
                {
                    return Err(error("KDE requires a Wayland session"));
                }
                let connection = application
                    .dbus_connection()
                    .ok_or_else(|| error("Application has no D-Bus connection"))?;
                return kde::Bridge::connect(connection).await.map(Self::Kde);
            }
            if name.eq_ignore_ascii_case("GNOME") {
                return gnome::Bridge::connect().await.map(Self::Gnome);
            }
        }
        Err(error(&format!(
            "Unsupported desktop {desktop:?}; select GNOME or KDE Wayland"
        )))
    }

    pub fn setup_hint(&self) -> &'static str {
        match self {
            Self::Gnome(_) => "Ensure the WayToClip GNOME bridge is enabled and available.",
            Self::Kde(_) => {
                "Ensure the WayToClip KWin script is enabled and KDE Wayland protocols are available."
            }
        }
    }

    pub async fn begin(&self) -> Result<PopupTarget, glib::Error> {
        match self {
            Self::Gnome(b) => b.begin().await,
            Self::Kde(b) => b.begin().await,
        }
    }

    pub async fn prepare_finish(
        &self,
        window: &gtk::ApplicationWindow,
        token: u32,
        should_paste: bool,
    ) -> Result<(), glib::Error> {
        match self {
            Self::Kde(bridge) if should_paste => bridge.prepare_finish(window, token).await,
            _ => Ok(()),
        }
    }

    pub async fn finish(
        &self,
        token: u32,
        text: &str,
        should_paste: bool,
        is_terminal: bool,
    ) -> Result<(), glib::Error> {
        match self {
            Self::Gnome(b) => b.finish(token, text, should_paste, is_terminal).await,
            Self::Kde(b) => b.finish(token, text, should_paste, is_terminal).await,
        }
    }
    pub async fn cancel(&self, token: u32, should_restore_focus: bool) -> Result<(), glib::Error> {
        match self {
            Self::Gnome(b) => b.cancel(token, should_restore_focus).await,
            Self::Kde(b) => b.cancel(token, should_restore_focus).await,
        }
    }
    pub async fn read_clipboard(&self) -> Result<String, glib::Error> {
        match self {
            Self::Gnome(b) => b.read_clipboard().await,
            Self::Kde(b) => b.read_clipboard().await,
        }
    }
}

fn error(message: &str) -> glib::Error {
    glib::Error::new(gio::IOErrorEnum::Failed, message)
}
