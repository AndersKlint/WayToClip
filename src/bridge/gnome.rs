use glib::variant::ToVariant;
use gtk::{gio, glib};

const BUS_NAME: &str = "org.gnome.Shell.Extensions.WayToClipBridge";
const OBJECT_PATH: &str = "/org/gnome/Shell/Extensions/WayToClipBridge";

use super::{ClipboardOffer, PopupTarget, error};

pub struct Bridge {
    connection: gio::DBusConnection,
}

impl Bridge {
    pub async fn connect() -> Result<Self, glib::Error> {
        let connection = gio::bus_get_future(gio::BusType::Session).await?;
        Ok(Self { connection })
    }

    async fn call(
        &self,
        method: &str,
        parameters: glib::Variant,
        reply_signature: &str,
        timeout_ms: i32,
    ) -> Result<glib::Variant, glib::Error> {
        self.connection
            .call_future(
                Some(BUS_NAME),
                OBJECT_PATH,
                BUS_NAME,
                method,
                Some(&parameters),
                Some(glib::VariantTy::new(reply_signature).unwrap()),
                gio::DBusCallFlags::NONE,
                timeout_ms,
            )
            .await
    }

    pub async fn configure_shortcuts(
        &self,
        bindings: &[(String, Vec<String>)],
    ) -> Result<(), glib::Error> {
        self.call("ConfigureShortcuts", (bindings,).to_variant(), "()", 10_000)
            .await?;
        Ok(())
    }

    pub async fn begin(&self, popup_position_mode: i32) -> Result<PopupTarget, glib::Error> {
        let reply = self
            .call(
                "BeginPopup",
                (popup_position_mode,).to_variant(),
                "(ussbbii)",
                10_000,
            )
            .await?;
        let (token, app_id, wm_class, is_terminal, has_target, available_width, available_height) =
            reply
                .get::<(u32, String, String, bool, bool, i32, i32)>()
                .ok_or_else(|| error("Invalid BeginPopup reply from the GNOME bridge"))?;
        Ok(PopupTarget {
            token,
            app_id,
            wm_class,
            is_terminal,
            has_target,
            available_width,
            available_height,
        })
    }

    pub async fn settle_popup(
        &self,
        token: u32,
        width: i32,
        height: i32,
    ) -> Result<i32, glib::Error> {
        if width <= 0 || height <= 0 {
            return Err(error(
                "GNOME popup settle requires positive width and height",
            ));
        }
        let reply = self
            .call(
                "SettlePopup",
                (token, width, height).to_variant(),
                "(i)",
                10_000,
            )
            .await?;
        reply
            .get::<(i32,)>()
            .map(|reply| reply.0)
            .ok_or_else(|| error("Invalid SettlePopup reply from the GNOME bridge"))
    }

    pub async fn prepare_finish(
        &self,
        _window: &gtk::ApplicationWindow,
        _token: u32,
        _should_paste: bool,
    ) -> Result<(), glib::Error> {
        Ok(())
    }

    pub async fn finish(
        &self,
        token: u32,
        mimetype: &str,
        bytes: &[u8],
        should_paste: bool,
        is_terminal: bool,
    ) -> Result<(), glib::Error> {
        self.call(
            "FinishPopup",
            (token, mimetype, bytes, should_paste, is_terminal).to_variant(),
            "()",
            10_000,
        )
        .await?;
        Ok(())
    }

    pub async fn cancel(&self, token: u32, should_restore_focus: bool) -> Result<(), glib::Error> {
        self.call(
            "CancelPopup",
            (token, should_restore_focus).to_variant(),
            "()",
            10_000,
        )
        .await?;
        Ok(())
    }

    pub async fn watch_clipboard(&self, after_serial: u64) -> Result<ClipboardOffer, glib::Error> {
        loop {
            let reply = match self
                .call(
                    "WatchClipboard",
                    (after_serial,).to_variant(),
                    "(tasssb)",
                    35_000,
                )
                .await
            {
                Ok(reply) => reply,
                Err(error) if error.message().contains("WatchTimeout") => continue,
                Err(error) => return Err(error),
            };
            let (serial, mimetypes, app_id, wm_class, is_own) = reply
                .get::<(u64, Vec<String>, String, String, bool)>()
                .ok_or_else(|| error("Invalid WatchClipboard reply from the GNOME bridge"))?;
            return Ok(ClipboardOffer {
                serial,
                mimetypes,
                app_id,
                wm_class,
                is_own,
            });
        }
    }

    pub async fn read_clipboard_content(
        &self,
        serial: u64,
        mimetype: &str,
    ) -> Result<Vec<u8>, glib::Error> {
        let reply = self
            .call(
                "ReadClipboardContent",
                (serial, mimetype).to_variant(),
                "(ay)",
                10_000,
            )
            .await?;
        let content = reply.child_value(0);
        let bytes = content
            .fixed_array::<u8>()
            .map_err(|_| error("Invalid ReadClipboardContent reply from the GNOME bridge"))?;
        if bytes.len() > super::MAX_CLIPBOARD_BYTES {
            return Err(error("Clipboard payload exceeds 16 MiB"));
        }
        Ok(bytes.to_vec())
    }

    pub async fn write_clipboard(&self, mimetype: &str, bytes: &[u8]) -> Result<(), glib::Error> {
        self.call(
            "WriteClipboard",
            (mimetype, bytes).to_variant(),
            "()",
            10_000,
        )
        .await?;
        Ok(())
    }
}
