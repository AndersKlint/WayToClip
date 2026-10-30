mod transport;

use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use gtk::{gdk, gio, glib, prelude::*};

use super::{PopupTarget, error};
use crate::clipboard::kde::KdeWayland;
use transport::Transport;

pub struct Bridge {
    transport: Transport,
    native: KdeWayland,
}

impl Bridge {
    pub async fn connect(connection: gio::DBusConnection) -> Result<Self, glib::Error> {
        let transport = Transport::new(connection)?;
        let native = KdeWayland::connect().await?;
        Ok(Self { transport, native })
    }

    pub async fn begin(&self) -> Result<PopupTarget, glib::Error> {
        self.transport.begin().await
    }

    pub async fn prepare_finish(
        &self,
        window: &gtk::ApplicationWindow,
        token: u32,
    ) -> Result<(), glib::Error> {
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

    pub async fn finish(
        &self,
        token: u32,
        text: &str,
        should_paste: bool,
        is_terminal: bool,
    ) -> Result<(), glib::Error> {
        let result = async {
            self.transport.validate(token)?;
            self.native.set_text(text).await?;
            self.transport.validate(token)?;
            self.transport
                .command("restore", token, serde_json::json!({}))
                .await?;
            self.transport.validate(token)?;
            if should_paste {
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

    pub async fn read_clipboard(&self) -> Result<String, glib::Error> {
        self.native.read_text().await
    }
}
