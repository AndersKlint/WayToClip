use glib::variant::ToVariant;
use gtk::{gio, glib};

const BUS_NAME: &str = "org.gnome.Shell.Extensions.WayToClipBridge";
const OBJECT_PATH: &str = "/org/gnome/Shell/Extensions/WayToClipBridge";

use super::PopupTarget;

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
                10_000,
            )
            .await
    }

    pub async fn begin(&self) -> Result<PopupTarget, glib::Error> {
        let reply = self.call("BeginPopup", ().to_variant(), "(usb)").await?;
        let (token, app_id, is_terminal) = reply.get::<(u32, String, bool)>().unwrap();
        Ok(PopupTarget {
            token,
            app_id,
            is_terminal,
        })
    }

    pub async fn finish(
        &self,
        token: u32,
        text: &str,
        should_paste: bool,
        is_terminal: bool,
    ) -> Result<(), glib::Error> {
        self.call(
            "FinishPopup",
            (token, text, should_paste, is_terminal).to_variant(),
            "()",
        )
        .await?;
        Ok(())
    }

    pub async fn cancel(&self, token: u32, should_restore_focus: bool) -> Result<(), glib::Error> {
        self.call(
            "CancelPopup",
            (token, should_restore_focus).to_variant(),
            "()",
        )
        .await?;
        Ok(())
    }

    pub async fn read_clipboard(&self) -> Result<String, glib::Error> {
        let reply = self.call("ReadClipboard", ().to_variant(), "(s)").await?;
        Ok(reply.get::<(String,)>().unwrap().0)
    }
}
