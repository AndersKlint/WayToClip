use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk::{gdk, gio, glib, prelude::*};

use crate::{bridge::Bridge, popup::Popup};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Beginning,
    Visible(u32),
    Reading(u32),
    Finishing,
    Cancelling,
}

pub struct Session {
    application: glib::WeakRef<gtk::Application>,
    popup: Popup,
    bridge: RefCell<Option<Rc<Bridge>>>,
    read_task: RefCell<Option<glib::JoinHandle<()>>>,
    boundary_task: RefCell<Option<glib::JoinHandle<()>>>,
    phase: Cell<Phase>,
    generation: Cell<u64>,
    has_activated: Cell<bool>,
    should_quit: Cell<bool>,
}

impl Session {
    pub fn new(application: &gtk::Application) -> Rc<Self> {
        let session = Rc::new(Self {
            application: application.downgrade(),
            popup: Popup::new(application),
            bridge: RefCell::new(None),
            read_task: RefCell::new(None),
            boundary_task: RefCell::new(None),
            phase: Cell::new(Phase::Idle),
            generation: Cell::new(0),
            has_activated: Cell::new(false),
            should_quit: Cell::new(false),
        });
        session.connect_signals();
        session
    }

    fn connect_signals(self: &Rc<Self>) {
        for (button, should_paste) in [(&self.popup.paste, true), (&self.popup.copy, false)] {
            let weak_session = Rc::downgrade(self);
            button.connect_clicked(move |_| {
                if let Some(session) = weak_session.upgrade() {
                    session.finish(should_paste);
                }
            });
        }
        let weak_session = Rc::downgrade(self);
        self.popup.read.connect_clicked(move |_| {
            if let Some(session) = weak_session.upgrade() {
                session.read_clipboard();
            }
        });
        let weak_session = Rc::downgrade(self);
        self.popup.quit.connect_clicked(move |_| {
            if let Some(session) = weak_session.upgrade() {
                session.quit();
            }
        });
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak_session = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                if let Some(session) = weak_session.upgrade() {
                    session.cancel(true);
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        self.popup.window.add_controller(keys);
        let weak_session = Rc::downgrade(self);
        self.popup.window.connect_is_active_notify(move |window| {
            if let Some(session) = weak_session.upgrade() {
                if !matches!(session.phase.get(), Phase::Visible(_) | Phase::Reading(_)) {
                    return;
                }
                if window.is_active() {
                    session.has_activated.set(true);
                } else if session.has_activated.get() {
                    session.cancel(false);
                }
            }
        });
        let weak_session = Rc::downgrade(self);
        self.popup.window.connect_close_request(move |_| {
            if let Some(session) = weak_session.upgrade() {
                session.cancel(true);
            }
            glib::Propagation::Stop
        });
    }

    pub fn activate(self: &Rc<Self>) {
        match self.phase.get() {
            Phase::Visible(_) | Phase::Reading(_) => {
                self.cancel(true);
                return;
            }
            Phase::Idle => {}
            _ => return,
        }
        if self.should_quit.get() {
            return;
        }
        self.phase.set(Phase::Beginning);
        let session = Rc::clone(self);
        let boundary_task = glib::spawn_future_local(async move {
            let cached_bridge = session.bridge.borrow().clone();
            let bridge = match cached_bridge {
                Some(bridge) => bridge,
                None => match Bridge::connect().await {
                    Ok(bridge) => {
                        let bridge = Rc::new(bridge);
                        session.bridge.replace(Some(Rc::clone(&bridge)));
                        bridge
                    }
                    Err(error) => {
                        session.report_error(&format!(
                            "Cannot connect to the session bus. Popup was not opened: {error}"
                        ));
                        session.complete();
                        return;
                    }
                },
            };
            match bridge.begin().await {
                Ok(target) => {
                    session.phase.set(Phase::Visible(target.token));
                    if session.should_quit.get() {
                        session.cancel(true);
                        return;
                    }
                    session.has_activated.set(false);
                    session.popup.prepare(&target.app_id, target.is_terminal);
                    session.popup.window.present();
                    session.popup.editor.grab_focus();
                }
                Err(error) => {
                    session.report_error(&format!("BeginPopup failed. Ensure the WayToClip GNOME bridge is enabled and available. Popup was not opened: {error}"));
                    session.complete();
                }
            }
        });
        self.boundary_task.replace(Some(boundary_task));
    }

    fn finish(self: &Rc<Self>, should_paste: bool) {
        let Phase::Visible(token) = self.phase.get() else {
            return;
        };
        let text = self.popup.text();
        let is_terminal = self.popup.terminal.is_active();
        self.phase.set(Phase::Finishing);
        self.popup.window.set_visible(false);
        let bridge = self.bridge.borrow().as_ref().unwrap().clone();
        let session = Rc::clone(self);
        let boundary_task = glib::spawn_future_local(async move {
            if let Err(error) = bridge.finish(token, &text, should_paste, is_terminal).await {
                session.report_error(&format!("FinishPopup failed. Clipboard and paste outcome are unknown. The popup will remain hidden: {error}"));
            }
            session.complete();
        });
        self.boundary_task.replace(Some(boundary_task));
    }

    fn cancel(self: &Rc<Self>, should_restore_focus: bool) {
        let token = match self.phase.get() {
            Phase::Visible(token) | Phase::Reading(token) => token,
            _ => return,
        };
        self.phase.set(Phase::Cancelling);
        self.generation.set(self.generation.get().wrapping_add(1));
        if let Some(read_task) = self.read_task.take() {
            read_task.abort();
        }
        self.popup.window.set_visible(false);
        let bridge = self.bridge.borrow().as_ref().unwrap().clone();
        let session = Rc::clone(self);
        let boundary_task = glib::spawn_future_local(async move {
            if let Err(error) = bridge.cancel(token, should_restore_focus).await {
                session.report_error(&format!("CancelPopup failed. The popup is hidden, but bridge focus restoration could not be confirmed: {error}"));
            }
            session.complete();
        });
        self.boundary_task.replace(Some(boundary_task));
    }

    fn read_clipboard(self: &Rc<Self>) {
        let Phase::Visible(token) = self.phase.get() else {
            return;
        };
        self.phase.set(Phase::Reading(token));
        self.popup.set_busy(true);
        self.popup.set_status("Reading clipboard…");
        let generation = self.generation.get();
        let bridge = self.bridge.borrow().as_ref().unwrap().clone();
        let session = Rc::clone(self);
        let read_task = glib::spawn_future_local(async move {
            let result = bridge.read_clipboard().await;
            if session.generation.get() != generation
                || session.phase.get() != Phase::Reading(token)
            {
                return;
            }
            session.read_task.take();
            session.phase.set(Phase::Visible(token));
            session.popup.set_busy(false);
            match result {
                Ok(text) => {
                    session.popup.editor.buffer().set_text(&text);
                    session.popup.set_status("");
                }
                Err(error) => {
                    let message = format!("ReadClipboard failed: {error}");
                    session.popup.set_status(&message);
                    session.report_error(&message);
                }
            }
        });
        self.read_task.replace(Some(read_task));
    }

    pub fn quit(self: &Rc<Self>) {
        self.should_quit.set(true);
        match self.phase.get() {
            Phase::Visible(_) | Phase::Reading(_) => self.cancel(true),
            Phase::Idle => self.complete(),
            _ => {}
        }
    }

    pub fn shutdown(&self) {
        self.should_quit.set(true);
        self.phase.set(Phase::Idle);
        self.generation.set(self.generation.get().wrapping_add(1));
        if let Some(read_task) = self.read_task.take() {
            read_task.abort();
        }
        if let Some(boundary_task) = self.boundary_task.take() {
            boundary_task.abort();
        }
        self.popup.window.destroy();
        self.bridge.take();
    }

    fn complete(&self) {
        self.phase.set(Phase::Idle);
        if self.should_quit.get() {
            if let Some(application) = self.application.upgrade() {
                application.quit();
            }
        }
    }

    fn report_error(&self, message: &str) {
        eprintln!("WayToClip: {message}");
        if let Some(application) = self.application.upgrade() {
            let notification = gio::Notification::new("WayToClip bridge error");
            notification.set_body(Some(message));
            application.send_notification(Some("bridge-error"), &notification);
        }
    }
}
