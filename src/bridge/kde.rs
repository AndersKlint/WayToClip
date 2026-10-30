mod shortcuts;
mod transport;

use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use gtk::{gdk, gio, glib, prelude::*};

use super::{ClipboardOffer, PopupTarget, error};
use crate::clipboard::kde::{KdeWayland, focused_app_handle};
use transport::Transport;

pub struct Bridge {
    transport: Transport,
    native: KdeWayland,
    shortcuts: shortcuts::Shortcuts,
}

impl Bridge {
    pub async fn connect(connection: gio::DBusConnection) -> Result<Self, glib::Error> {
        let focused = focused_app_handle();
        let shortcuts = shortcuts::Shortcuts::new(connection.clone());
        let transport = Transport::new(connection, focused.clone())?;
        let native = KdeWayland::connect(focused).await?;
        Ok(Self {
            transport,
            native,
            shortcuts,
        })
    }

    pub async fn configure_shortcuts(
        &self,
        bindings: &[(String, Vec<String>)],
    ) -> Result<(), glib::Error> {
        self.shortcuts.configure(bindings).await
    }

    pub async fn begin(&self, popup_position_mode: i32) -> Result<PopupTarget, glib::Error> {
        if !matches!(popup_position_mode, 0 | 1) {
            return Err(error("Unsupported KDE popup position mode"));
        }
        self.transport.begin(popup_position_mode).await
    }

    pub async fn prepare_finish(
        &self,
        window: &gtk::ApplicationWindow,
        token: u32,
        should_paste: bool,
    ) -> Result<(), glib::Error> {
        if !should_paste {
            return Ok(());
        }
        if !self.transport.has_target(token)? {
            return Err(error(
                "KDE paste preparation failed: no eligible target window",
            ));
        }
        let deadline = Instant::now() + Duration::from_millis(1500);
        let has_lost_focus = Rc::new(Cell::new(!window.is_active()));
        let focus_state = Rc::clone(&has_lost_focus);
        let focus_watch = glib::SignalGroup::new::<gtk::ApplicationWindow>();
        focus_watch.connect_notify_local(Some("is-active"), move |window, _| {
            if !window.property::<bool>("is-active") {
                focus_state.set(true);
            }
        });
        focus_watch.set_target(Some(window));
        let modifiers = gdk::ModifierType::CONTROL_MASK
            | gdk::ModifierType::SHIFT_MASK
            | gdk::ModifierType::ALT_MASK
            | gdk::ModifierType::SUPER_MASK
            | gdk::ModifierType::META_MASK
            | gdk::ModifierType::HYPER_MASK;
        loop {
            self.transport.validate(token)?;
            if has_lost_focus.get() || !window.is_active() {
                return Err(error("KDE paste preparation failed: popup lost focus"));
            }
            let keyboard = WidgetExt::display(window)
                .default_seat()
                .and_then(|seat| seat.keyboard())
                .ok_or_else(|| {
                    error(
                        "KDE paste preparation failed: GTK keyboard modifier state is unavailable",
                    )
                })?;
            if Instant::now() >= deadline {
                return Err(error(
                    "KDE paste preparation timed out: release Control, Shift, Alt, Super, Meta and Hyper within 1.5 seconds",
                ));
            }
            if !keyboard.modifier_state().intersects(modifiers) {
                return self.transport.validate(token);
            }
            glib::timeout_future(Duration::from_millis(20)).await;
        }
    }

    pub async fn settle_popup(
        &self,
        token: u32,
        width: i32,
        height: i32,
    ) -> Result<i32, glib::Error> {
        if width <= 0 || height <= 0 {
            return Err(error("KDE popup settle requires positive width and height"));
        }
        let reply = self
            .transport
            .command(
                "settle",
                token,
                serde_json::json!({"width": width, "height": height}),
            )
            .await?;
        let available_height =
            transport::required_i32(&reply["available_height"], "available_height")?;
        if available_height < 0 {
            return Err(error("KWin settle result has an invalid available height"));
        }
        Ok(available_height)
    }

    pub async fn finish(
        &self,
        token: u32,
        mimetype: &str,
        bytes: &[u8],
        should_paste: bool,
        is_terminal: bool,
    ) -> Result<(), glib::Error> {
        if should_paste && !self.transport.has_target(token)? {
            return Err(error("KDE paste failed: no eligible target window"));
        }
        let result = async {
            self.transport.validate(token)?;
            self.native.write(mimetype, bytes.to_vec()).await?;
            self.transport.validate(token)?;
            self.transport
                .command("restore", token, serde_json::json!({}))
                .await?;
            self.transport.validate(token)?;
            if should_paste {
                if !self.transport.has_target(token)? {
                    return Err(error("KDE paste failed: target window was lost"));
                }
                self.native.paste(is_terminal).await?;
                self.transport.validate(token)?;
            }
            self.transport
                .command("complete", token, serde_json::json!({}))
                .await?;
            Ok(())
        }
        .await;
        if result.is_err() && self.transport.validate(token).is_ok() {
            let _ = self
                .transport
                .command(
                    "cancel",
                    token,
                    serde_json::json!({"should_restore_focus": false}),
                )
                .await;
        }
        self.transport.clear(token, "Popup finished");
        result
    }

    pub async fn cancel(&self, token: u32, should_restore_focus: bool) -> Result<(), glib::Error> {
        let result = self
            .transport
            .command(
                "cancel",
                token,
                serde_json::json!({"should_restore_focus": should_restore_focus}),
            )
            .await;
        self.transport.clear(token, "Popup cancelled");
        result.map(|_| ())
    }

    pub async fn watch_clipboard(&self, after_serial: u64) -> Result<ClipboardOffer, glib::Error> {
        loop {
            let offer = self.native.watch(after_serial).await?;
            if offer.serial == after_serial {
                continue;
            }
            return Ok(ClipboardOffer {
                serial: offer.serial,
                mimetypes: offer.mimetypes,
                app_id: offer.app_id,
                wm_class: offer.wm_class,
                is_own: offer.is_own,
            });
        }
    }

    pub async fn read_clipboard_content(
        &self,
        serial: u64,
        mimetype: &str,
    ) -> Result<Vec<u8>, glib::Error> {
        self.native.read(serial, mimetype).await
    }

    pub async fn write_clipboard(&self, mimetype: &str, bytes: &[u8]) -> Result<(), glib::Error> {
        self.native.write(mimetype, bytes.to_vec()).await
    }
}
