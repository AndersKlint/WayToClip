mod gnome;
mod kde;

use gtk::{gio, glib, prelude::*};

const MAX_CLIPBOARD_BYTES: usize = 16 * 1024 * 1024;

pub struct ClipboardOffer {
    pub serial: u64,
    pub mimetypes: Vec<String>,
    pub app_id: String,
    pub wm_class: String,
    pub is_own: bool,
}

pub struct PopupTarget {
    pub token: u32,
    pub app_id: String,
    pub wm_class: String,
    pub is_terminal: bool,
    pub has_target: bool,
    pub available_width: i32,
    pub available_height: i32,
}

pub enum Bridge {
    Gnome(gnome::Bridge),
    Kde(kde::Bridge),
}

impl Bridge {
    pub async fn configure_shortcuts(
        &self,
        bindings: &[(String, Vec<String>)],
    ) -> Result<(), glib::Error> {
        match self {
            Self::Gnome(bridge) => bridge.configure_shortcuts(bindings).await,
            Self::Kde(bridge) => bridge.configure_shortcuts(bindings).await,
        }
    }

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

    pub async fn begin(&self, popup_position_mode: i32) -> Result<PopupTarget, glib::Error> {
        if !matches!(popup_position_mode, 0 | 1) {
            return Err(error("Unsupported popup position mode"));
        }
        match self {
            Self::Gnome(b) => b.begin(popup_position_mode).await,
            Self::Kde(b) => b.begin(popup_position_mode).await,
        }
    }

    pub async fn settle_popup(
        &self,
        token: u32,
        width: i32,
        height: i32,
    ) -> Result<i32, glib::Error> {
        if width <= 0 || height <= 0 {
            return Err(error("Popup settle requires positive width and height"));
        }
        match self {
            Self::Gnome(b) => b.settle_popup(token, width, height).await,
            Self::Kde(b) => b.settle_popup(token, width, height).await,
        }
    }

    pub async fn prepare_finish(
        &self,
        window: &gtk::ApplicationWindow,
        token: u32,
        should_paste: bool,
    ) -> Result<(), glib::Error> {
        match self {
            Self::Gnome(b) => b.prepare_finish(window, token, should_paste).await,
            Self::Kde(b) => b.prepare_finish(window, token, should_paste).await,
        }
    }

    pub async fn finish(
        &self,
        token: u32,
        mimetype: &str,
        bytes: &[u8],
        should_paste: bool,
        is_terminal: bool,
    ) -> Result<(), glib::Error> {
        validate_clipboard_content(mimetype, bytes)?;
        match self {
            Self::Gnome(b) => {
                b.finish(token, mimetype, bytes, should_paste, is_terminal)
                    .await
            }
            Self::Kde(b) => {
                b.finish(token, mimetype, bytes, should_paste, is_terminal)
                    .await
            }
        }
    }

    pub async fn cancel(&self, token: u32, should_restore_focus: bool) -> Result<(), glib::Error> {
        match self {
            Self::Gnome(b) => b.cancel(token, should_restore_focus).await,
            Self::Kde(b) => b.cancel(token, should_restore_focus).await,
        }
    }

    pub async fn watch_clipboard(&self, after_serial: u64) -> Result<ClipboardOffer, glib::Error> {
        match self {
            Self::Gnome(b) => b.watch_clipboard(after_serial).await,
            Self::Kde(b) => b.watch_clipboard(after_serial).await,
        }
    }

    pub async fn read_clipboard_content(
        &self,
        serial: u64,
        mimetype: &str,
    ) -> Result<Vec<u8>, glib::Error> {
        match self {
            Self::Gnome(b) => b.read_clipboard_content(serial, mimetype).await,
            Self::Kde(b) => b.read_clipboard_content(serial, mimetype).await,
        }
    }

    pub async fn write_clipboard(&self, mimetype: &str, bytes: &[u8]) -> Result<(), glib::Error> {
        validate_clipboard_content(mimetype, bytes)?;
        match self {
            Self::Gnome(b) => b.write_clipboard(mimetype, bytes).await,
            Self::Kde(b) => b.write_clipboard(mimetype, bytes).await,
        }
    }
}

fn validate_clipboard_content(mimetype: &str, bytes: &[u8]) -> Result<(), glib::Error> {
    if mimetype.is_empty() || mimetype.len() > 4096 || mimetype.contains('\0') {
        return Err(error("Invalid clipboard MIME type"));
    }
    if bytes.len() > MAX_CLIPBOARD_BYTES {
        return Err(error("Clipboard payload exceeds 16 MiB"));
    }
    Ok(())
}

fn error(message: &str) -> glib::Error {
    glib::Error::new(gio::IOErrorEnum::Failed, message)
}
