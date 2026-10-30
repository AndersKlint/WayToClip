use std::{
    cell::{Cell, RefCell},
    future::Future,
    rc::Rc,
    time::Duration,
};

use gtk::{gio, glib, prelude::*};

use crate::{
    bridge::Bridge,
    clipboard::{
        clipboard_entry::ClipboardEntry,
        clipboard_manager::{CapturedClipboard, ClipboardManager, ClipboardPolicy},
    },
    common::i18n::{sync_override_from_settings, translate_native},
    cursor_popup::{CursorPopup, PopupAction},
    history::{
        history_clear_scheduler::HistoryClearScheduler,
        history_store::{HistoryStore, RemovedEntry},
        registry::Registry,
    },
    paste::paste_target::is_terminal_window,
    session::{Phase, Session},
    settings::{SettingsManager, SettingsSnapshot, SettingsWindow},
};

const RECONNECT_DELAY: Duration = Duration::from_secs(2);

struct AuxiliaryTask {
    id: u64,
    task: glib::JoinHandle<()>,
}

struct AuxiliaryGuard {
    id: u64,
    tasks: Rc<RefCell<Vec<AuxiliaryTask>>>,
}

impl Drop for AuxiliaryGuard {
    fn drop(&mut self) {
        self.tasks.borrow_mut().retain(|task| task.id != self.id);
    }
}

#[derive(Clone)]
struct OpenTarget {
    app_id: String,
    wm_class: String,
    is_terminal: bool,
    has_target: bool,
}

pub struct WayToClipController {
    application: glib::WeakRef<gtk::Application>,
    settings: Rc<SettingsManager>,
    snapshot: RefCell<SettingsSnapshot>,
    registry: Rc<Registry>,
    store: RefCell<HistoryStore>,
    popup: Rc<CursorPopup>,
    clipboard: Rc<ClipboardManager>,
    scheduler: Rc<HistoryClearScheduler>,
    session: Session,
    bridge: RefCell<Option<Rc<Bridge>>>,
    target: RefCell<Option<OpenTarget>>,
    is_private_mode: Cell<bool>,
    is_running: Cell<bool>,
    is_booted: Cell<bool>,
    pending_activate: Cell<bool>,
    pending_clear: Cell<bool>,
    pending_reset_timer: Cell<bool>,
    opening_toggle: Cell<bool>,
    is_clear_dialog_open: Cell<bool>,
    clear_dialog_task: RefCell<Option<glib::JoinHandle<()>>>,
    settings_handler: RefCell<Option<glib::SignalHandlerId>>,
    backend_task: RefCell<Option<glib::JoinHandle<()>>>,
    boot_task: RefCell<Option<glib::JoinHandle<()>>>,
    auxiliary_tasks: Rc<RefCell<Vec<AuxiliaryTask>>>,
    next_auxiliary_id: Cell<u64>,
    settings_window: RefCell<Option<Rc<SettingsWindow>>>,
    should_present_settings: Cell<bool>,
    is_shortcut_capture_active: Cell<bool>,
    shortcut_revision: Cell<u64>,
    shortcut_task: RefCell<Option<glib::JoinHandle<()>>>,
    shortcut_status: RefCell<String>,
}

impl WayToClipController {
    pub fn new(application: &gtk::Application) -> Result<Rc<Self>, glib::Error> {
        let settings = Rc::new(SettingsManager::new()?);
        sync_override_from_settings(settings.gio());
        let snapshot = settings.snapshot();
        let registry = Registry::new();
        let popup = CursorPopup::new(application);
        let clipboard = ClipboardManager::new();
        let scheduler = HistoryClearScheduler::new(Rc::clone(&settings));
        let controller = Rc::new(Self {
            application: application.downgrade(),
            settings,
            snapshot: RefCell::new(snapshot),
            registry,
            store: RefCell::new(HistoryStore::new()),
            popup,
            clipboard,
            scheduler,
            session: Session::new(),
            bridge: RefCell::new(None),
            target: RefCell::new(None),
            is_private_mode: Cell::new(false),
            is_running: Cell::new(false),
            is_booted: Cell::new(false),
            pending_activate: Cell::new(false),
            pending_clear: Cell::new(false),
            pending_reset_timer: Cell::new(false),
            opening_toggle: Cell::new(false),
            is_clear_dialog_open: Cell::new(false),
            clear_dialog_task: RefCell::new(None),
            settings_handler: RefCell::new(None),
            backend_task: RefCell::new(None),
            boot_task: RefCell::new(None),
            auxiliary_tasks: Rc::new(RefCell::new(Vec::new())),
            next_auxiliary_id: Cell::new(0),
            settings_window: RefCell::new(None),
            should_present_settings: Cell::new(false),
            is_shortcut_capture_active: Cell::new(false),
            shortcut_revision: Cell::new(0),
            shortcut_task: RefCell::new(None),
            shortcut_status: RefCell::new("Unavailable: desktop backend is not connected".into()),
        });
        controller.wire();
        Ok(controller)
    }

    fn wire(self: &Rc<Self>) {
        let weak_controller = Rc::downgrade(self);
        self.popup.set_action_handler(Rc::new(move |action| {
            if let Some(controller) = weak_controller.upgrade() {
                controller.handle_popup_action(action);
            }
        }));

        let weak_controller = Rc::downgrade(self);
        self.clipboard
            .set_policy(Rc::new(move || match weak_controller.upgrade() {
                Some(controller) => {
                    let snapshot = controller.snapshot.borrow();
                    ClipboardPolicy {
                        is_private_mode: controller.is_private_mode.get(),
                        should_ignore_secrets: snapshot.should_ignore_secrets,
                        should_cache_images: snapshot.should_cache_images,
                        excluded_apps: snapshot.excluded_apps.clone(),
                    }
                }
                None => ClipboardPolicy {
                    is_private_mode: true,
                    should_ignore_secrets: true,
                    should_cache_images: false,
                    excluded_apps: Vec::new(),
                },
            }));

        let weak_controller = Rc::downgrade(self);
        self.clipboard
            .set_handler(Rc::new(move |capture: CapturedClipboard| {
                if let Some(controller) = weak_controller.upgrade() {
                    controller.on_clipboard_capture(capture);
                }
            }));

        let weak_controller = Rc::downgrade(self);
        let handler = self.settings.gio().connect_changed(None, move |_, _| {
            if let Some(controller) = weak_controller.upgrade() {
                controller.on_settings_changed();
            }
        });
        self.settings_handler.replace(Some(handler));
    }

    pub fn start(self: &Rc<Self>) {
        if self.is_running.replace(true) {
            return;
        }
        let weak_controller = Rc::downgrade(self);
        let task = glib::spawn_future_local(async move {
            if let Some(controller) = weak_controller.upgrade() {
                controller.boot().await;
            }
        });
        self.boot_task.replace(Some(task));
    }

    pub fn open_settings(self: &Rc<Self>) {
        if !self.is_running.get() {
            return;
        }
        self.pending_activate.set(false);
        self.should_present_settings.set(true);
        match self.session.phase() {
            Phase::Visible(_) => self.close_popup(false),
            Phase::Opening(_) => self.opening_toggle.set(true),
            Phase::Idle => self.present_pending_settings(),
            Phase::Finishing(_) | Phase::Cancelling(_) => {}
        }
    }

    fn present_pending_settings(&self) {
        if !self.is_running.get()
            || !self.should_present_settings.get()
            || !matches!(self.session.phase(), Phase::Idle)
        {
            return;
        }
        let Some(application) = self.application.upgrade() else {
            return;
        };
        if self.settings_window.borrow().is_none() {
            let window = SettingsWindow::new(&application, Rc::clone(&self.settings));
            window.set_shortcut_status(&self.shortcut_status.borrow());
            self.settings_window.replace(Some(window));
        }
        self.should_present_settings.set(false);
        if let Some(window) = self.settings_window.borrow().as_ref() {
            window.present();
        }
    }

    pub fn set_shortcut_capture(self: &Rc<Self>, is_active: bool) {
        if !self.is_running.get() || self.is_shortcut_capture_active.replace(is_active) == is_active
        {
            return;
        }
        self.pending_activate.set(false);
        self.set_capture_ready(false);
        self.configure_shortcuts();
    }

    fn set_capture_ready(&self, is_ready: bool) {
        if let Some(application) = self.application.upgrade() {
            if let Some(action) = application
                .lookup_action("shortcut-capture-ready")
                .and_then(|action| action.downcast::<gio::SimpleAction>().ok())
            {
                action.set_state(&is_ready.to_variant());
            }
        }
    }

    fn set_shortcut_status(&self, status: &str) {
        self.shortcut_status.replace(status.to_owned());
        if let Some(window) = self.settings_window.borrow().as_ref() {
            window.set_shortcut_status(status);
        }
    }

    fn stop_shortcut_task(&self) {
        self.set_capture_ready(false);
        if let Some(task) = self.shortcut_task.borrow_mut().take() {
            task.abort();
        }
    }

    pub fn refresh_shortcuts(self: &Rc<Self>) {
        if self.is_running.get() {
            self.configure_shortcuts();
        }
    }

    fn configure_shortcuts(self: &Rc<Self>) {
        self.shortcut_revision
            .set(self.shortcut_revision.get().wrapping_add(1));
        if self.bridge.borrow().is_none() {
            self.set_shortcut_status("Unavailable: desktop backend is not connected");
            return;
        }
        self.set_shortcut_status("Pending: configuring global shortcuts…");
        if self.shortcut_task.borrow().is_some() {
            return;
        }
        let weak_controller = Rc::downgrade(self);
        let task = glib::spawn_future_local(async move {
            loop {
                let Some(controller) = weak_controller.upgrade() else {
                    return;
                };
                let Some(bridge) = controller.bridge.borrow().clone() else {
                    return;
                };
                let revision = controller.shortcut_revision.get();
                let is_capture = controller.is_shortcut_capture_active.get();
                let bindings = {
                    let snapshot = controller.snapshot.borrow();
                    if is_capture || !snapshot.is_keybinding_enabled {
                        Vec::new()
                    } else {
                        [
                            ("toggle-popup", &snapshot.toggle_popup_shortcut),
                            ("clear-history", &snapshot.clear_history_shortcut),
                            ("toggle-private-mode", &snapshot.private_mode_shortcut),
                        ]
                        .into_iter()
                        .filter(|(_, shortcuts)| !shortcuts.is_empty())
                        .map(|(action, shortcuts)| (action.to_owned(), shortcuts.clone()))
                        .collect()
                    }
                };
                let result = bridge.configure_shortcuts(&bindings).await;
                if revision != controller.shortcut_revision.get() {
                    continue;
                }
                match result {
                    Ok(()) => {
                        controller.set_capture_ready(is_capture);
                        controller.set_shortcut_status(if is_capture {
                            "Success: global shortcuts suspended for capture"
                        } else if bindings.is_empty() {
                            "Success: no global shortcuts registered"
                        } else {
                            "Success: global shortcuts registered"
                        });
                    }
                    Err(error) => {
                        controller.set_capture_ready(false);
                        controller
                            .set_shortcut_status(&format!("Conflict or unavailable: {error}"));
                    }
                }
                controller.shortcut_task.borrow_mut().take();
                return;
            }
        });
        self.shortcut_task.replace(Some(task));
    }

    async fn boot(self: &Rc<Self>) {
        sync_override_from_settings(self.settings.gio());
        let snapshot = self.snapshot.borrow().clone();
        if snapshot.should_clear_on_boot {
            if let Err(error) = self.registry.clear_cache_folder().await {
                self.report(&format!("Cannot clear the clipboard cache: {error}"));
            }
        }

        match self.registry.read(&snapshot).await {
            Ok(entries) => self.store.borrow_mut().load(entries),
            Err(error) => {
                self.store.borrow_mut().load(Vec::new());
                self.report(&format!("Cannot load clipboard history: {error}"));
            }
        }

        self.is_booted.set(true);
        if self.pending_clear.replace(false) {
            self.clear_history();
        }

        let weak_controller = Rc::downgrade(self);
        self.scheduler.start(move || {
            if let Some(controller) = weak_controller.upgrade() {
                controller.clear_history();
            }
        });

        if self.pending_reset_timer.replace(false) {
            self.scheduler.schedule();
        }

        self.spawn_backend();
        self.open_pending_activation();
    }

    fn spawn_backend(self: &Rc<Self>) {
        let weak_controller = Rc::downgrade(self);
        let task = glib::spawn_future_local(async move {
            let mut last_reported: Option<String> = None;
            loop {
                let Some(controller) = weak_controller.upgrade() else {
                    return;
                };
                if !controller.is_running.get() {
                    return;
                }
                let Some(application) = controller.application.upgrade() else {
                    return;
                };

                match Bridge::connect(&application).await {
                    Ok(bridge) => {
                        let bridge = Rc::new(bridge);
                        controller.bridge.replace(Some(Rc::clone(&bridge)));
                        controller.configure_shortcuts();
                        controller.open_pending_activation();
                        let result = controller.clipboard.run(Rc::clone(&bridge)).await;
                        match controller.session.phase() {
                            Phase::Visible(_) => controller.close_popup(false),
                            Phase::Opening(_) => {
                                controller.opening_toggle.set(false);
                                controller.popup.close();
                            }
                            Phase::Idle | Phase::Finishing(_) | Phase::Cancelling(_) => {}
                        }
                        controller.stop_shortcut_task();
                        let _ = bridge.configure_shortcuts(&[]).await;
                        controller.bridge.take();
                        controller.set_shortcut_status("Unavailable: desktop backend disconnected");
                        controller.clipboard.invalidate_pending();
                        if !controller.is_running.get() {
                            return;
                        }
                        if let Err(error) = result {
                            let message = format!(
                                "Clipboard monitoring stopped. {}. Reconnecting: {error}",
                                bridge.setup_hint()
                            );
                            if last_reported.as_deref() != Some(message.as_str()) {
                                controller.report(&message);
                                last_reported = Some(message);
                            }
                        }
                    }
                    Err(error) => {
                        let message =
                            format!("Cannot initialize the desktop backend. Retrying: {error}");
                        if last_reported.as_deref() != Some(message.as_str()) {
                            controller.report(&message);
                            last_reported = Some(message);
                        }
                    }
                }

                glib::timeout_future(RECONNECT_DELAY).await;
            }
        });
        self.backend_task.replace(Some(task));
    }

    pub fn activate(self: &Rc<Self>) {
        if !self.is_running.get()
            || self.is_shortcut_capture_active.get()
            || self.should_present_settings.get()
        {
            return;
        }
        match self.session.phase() {
            Phase::Visible(_) => self.close_popup(true),
            Phase::Opening(_) => {
                self.opening_toggle.set(!self.opening_toggle.get());
            }
            Phase::Finishing(_) | Phase::Cancelling(_) => {}
            Phase::Idle => {
                if !self.is_booted.get() || self.bridge.borrow().is_none() {
                    self.pending_activate.set(true);
                    return;
                }
                self.open_popup();
            }
        }
    }

    fn open_pending_activation(self: &Rc<Self>) {
        self.present_pending_settings();
        if !self.is_running.get()
            || self.is_shortcut_capture_active.get()
            || self.should_present_settings.get()
            || !self.pending_activate.get()
            || !self.is_booted.get()
            || self.bridge.borrow().is_none()
            || !matches!(self.session.phase(), Phase::Idle)
        {
            return;
        }
        self.pending_activate.set(false);
        self.open_popup();
    }

    fn open_popup(self: &Rc<Self>) {
        if !self.is_running.get() || !matches!(self.session.phase(), Phase::Idle) {
            return;
        }
        let Some(bridge) = self.bridge.borrow().clone() else {
            self.pending_activate.set(true);
            return;
        };
        let snapshot = self.snapshot.borrow().clone();
        let entries = self.store.borrow().entries().to_vec();
        let is_private_mode = self.is_private_mode.get();
        let generation = self.session.begin_opening();
        self.opening_toggle.set(false);
        let weak_controller = Rc::downgrade(self);
        let task = glib::spawn_future_local(async move {
            let Some(controller) = weak_controller.upgrade() else {
                return;
            };
            let target = match bridge.begin(snapshot.popup_position_mode).await {
                Ok(target) => target,
                Err(error) => {
                    controller.opening_toggle.set(false);
                    if controller.session.is_opening(generation) {
                        controller.session.set_phase(Phase::Idle);
                        if controller.is_running.get() {
                            controller.report(&format!(
                                "Popup could not be opened. {} {error}",
                                bridge.setup_hint()
                            ));
                        }
                    }
                    controller.open_pending_activation();
                    return;
                }
            };
            if controller.should_cancel_opening(generation, &bridge) {
                let _ = bridge.cancel(target.token, false).await;
                controller.finish_opening(generation);
                return;
            }

            controller.popup.prepare(
                entries,
                &snapshot,
                target.available_width,
                target.available_height,
                is_private_mode,
            );
            let (width, height) = controller.popup.natural_size();
            let available_height = match bridge.settle_popup(target.token, width, height).await {
                Ok(available_height) => available_height,
                Err(error) => {
                    let should_report = !controller.should_cancel_opening(generation, &bridge);
                    controller.popup.close();
                    let _ = bridge.cancel(target.token, false).await;
                    controller.finish_opening(generation);
                    if should_report && controller.is_running.get() {
                        controller.report(&format!(
                            "Popup could not be opened. {} {error}",
                            bridge.setup_hint()
                        ));
                    }
                    return;
                }
            };
            if controller.should_cancel_opening(generation, &bridge) {
                controller.popup.close();
                let _ = bridge.cancel(target.token, false).await;
                controller.finish_opening(generation);
                return;
            }

            let is_terminal = target.is_terminal
                || is_terminal_window(
                    Some(&target.wm_class),
                    Some(&target.app_id),
                    &snapshot.terminal_apps,
                );
            controller.popup.constrain_height(available_height);
            controller.target.replace(Some(OpenTarget {
                app_id: target.app_id.clone(),
                wm_class: target.wm_class.clone(),
                is_terminal,
                has_target: target.has_target,
            }));
            controller.session.set_phase(Phase::Visible(target.token));
            controller.popup.present();
            controller.popup.set_busy(false);
            controller.popup.set_status("");
        });
        self.session.track_opening(task);
    }

    fn should_cancel_opening(self: &Rc<Self>, generation: u64, bridge: &Rc<Bridge>) -> bool {
        !self.is_running.get()
            || self.should_present_settings.get()
            || self.is_shortcut_capture_active.get()
            || !self.session.is_opening(generation)
            || !self
                .bridge
                .borrow()
                .as_ref()
                .is_some_and(|current| Rc::ptr_eq(current, bridge))
            || self.opening_toggle.get()
    }

    fn finish_opening(self: &Rc<Self>, generation: u64) {
        self.opening_toggle.set(false);
        if self.session.is_opening(generation) {
            self.session.set_phase(Phase::Idle);
        }
        self.open_pending_activation();
    }

    fn handle_popup_action(self: &Rc<Self>, action: PopupAction) {
        if !matches!(self.session.phase(), Phase::Visible(_)) {
            return;
        }
        match action {
            PopupAction::SelectEntry(entry) => self.select_entry(entry),
            PopupAction::RemoveEntry(entry) => self.remove_entry(entry),
            PopupAction::ToggleFavorite(entry) => self.toggle_favorite(entry),
            PopupAction::TogglePrivateMode => self.toggle_private_mode(),
            PopupAction::SetSearchOption { name, is_enabled } => {
                self.set_search_option(&name, is_enabled);
            }
            PopupAction::Close {
                should_restore_focus,
            } => self.close_popup(should_restore_focus),
        }
    }

    fn select_entry(self: &Rc<Self>, entry: ClipboardEntry) {
        let Phase::Visible(token) = self.session.phase() else {
            return;
        };
        let Some(target) = self.target.borrow().clone() else {
            return;
        };
        let (should_move_first, should_auto_paste, terminal_apps) = {
            let snapshot = self.snapshot.borrow();
            (
                snapshot.should_move_item_first,
                snapshot.should_auto_paste,
                snapshot.terminal_apps.clone(),
            )
        };
        let selected = {
            let mut store = self.store.borrow_mut();
            store.select(&entry, should_move_first)
        };
        let Some(selected) = selected else {
            self.close_popup(true);
            return;
        };
        self.persist();
        let is_terminal = target.is_terminal
            || is_terminal_window(Some(&target.wm_class), Some(&target.app_id), &terminal_apps);
        let should_paste = should_auto_paste && target.has_target;
        self.finish(
            token,
            selected.mimetype.clone(),
            selected.bytes.clone(),
            should_paste,
            is_terminal,
        );
    }

    fn finish(
        self: &Rc<Self>,
        token: u32,
        mimetype: String,
        bytes: Vec<u8>,
        should_paste: bool,
        is_terminal: bool,
    ) {
        if !matches!(self.session.phase(), Phase::Visible(current) if current == token) {
            return;
        }
        self.session.set_phase(Phase::Finishing(token));
        self.popup.set_busy(true);
        let Some(bridge) = self.bridge.borrow().clone() else {
            self.popup.close();
            self.complete();
            return;
        };
        let window = self.popup.window().clone();
        let weak_controller = Rc::downgrade(self);
        let task = glib::spawn_future_local(async move {
            let Some(controller) = weak_controller.upgrade() else {
                return;
            };
            if let Err(error) = bridge.prepare_finish(&window, token, should_paste).await {
                controller.popup.close();
                controller.report(&format!(
                    "Paste preparation failed. Clipboard was not changed and paste was not requested: {error}"
                ));
                let _ = bridge.cancel(token, false).await;
                controller.complete();
                return;
            }
            controller.popup.close();
            if let Err(error) = bridge
                .finish(token, &mimetype, &bytes, should_paste, is_terminal)
                .await
            {
                controller.report(&format!(
                    "Finishing the popup failed. Clipboard and paste outcome are unknown: {error}"
                ));
            }
            controller.complete();
        });
        self.session.track_boundary(task);
    }

    fn close_popup(self: &Rc<Self>, should_restore_focus: bool) {
        let Phase::Visible(token) = self.session.phase() else {
            return;
        };
        self.session.set_phase(Phase::Cancelling(token));
        self.popup.close();
        self.cancel_token(token, should_restore_focus);
    }

    fn cancel_token(self: &Rc<Self>, token: u32, should_restore_focus: bool) {
        let Some(bridge) = self.bridge.borrow().clone() else {
            self.complete();
            return;
        };
        let weak_controller = Rc::downgrade(self);
        let task = glib::spawn_future_local(async move {
            if let Err(error) = bridge.cancel(token, should_restore_focus).await {
                if let Some(controller) = weak_controller.upgrade() {
                    controller.report(&format!(
                        "Popup is hidden, but bridge focus restoration could not be confirmed: {error}"
                    ));
                }
            }
            if let Some(controller) = weak_controller.upgrade() {
                controller.complete();
            }
        });
        self.session.track_boundary(task);
    }

    fn complete(&self) {
        self.session.set_phase(Phase::Idle);
        self.target.replace(None);
        self.present_pending_settings();
    }

    fn on_clipboard_capture(self: &Rc<Self>, capture: CapturedClipboard) {
        let entry = ClipboardEntry::new(capture.mimetype, capture.bytes, false);
        if entry.is_image() {
            let write = self.registry.write_entry_file(&entry);
            let weak_controller = Rc::downgrade(self);
            self.spawn_auxiliary(async move {
                if let Err(error) = write.await {
                    if let Some(controller) = weak_controller.upgrade() {
                        controller.report(&format!("Cannot cache clipboard image: {error}"));
                    }
                }
            });
        }
        let existing = self.store.borrow().find_equal(&entry).cloned();
        if let Some(existing) = existing {
            self.store.borrow_mut().select(&existing, true);
            self.persist();
            return;
        }

        let removed = {
            let max_length = self.snapshot.borrow().max_registry_length;
            let mut store = self.store.borrow_mut();
            store.add(entry.clone());
            store.select(&entry, false);
            store.trim(max_length)
        };
        self.delete_removed_images(removed);
        self.persist();
    }

    fn remove_entry(self: &Rc<Self>, entry: ClipboardEntry) {
        let removed = self.store.borrow_mut().remove(&entry);
        let Some(removed) = removed else {
            return;
        };
        if removed.was_selected {
            self.clear_clipboard();
        }
        self.delete_removed_images(vec![removed]);
        self.persist();
        self.refresh_popup_entries();
    }

    fn toggle_favorite(self: &Rc<Self>, entry: ClipboardEntry) {
        if !self.snapshot.borrow().is_favorites_enabled {
            return;
        }
        let live = self.store.borrow().find_equal(&entry).cloned();
        let Some(live) = live else {
            return;
        };
        self.store.borrow_mut().toggle_favorite(&live);
        self.persist();
        self.refresh_popup_entries();
    }

    pub fn request_clear_history(self: &Rc<Self>) {
        if !self.is_running.get() || self.is_private_mode.get() {
            return;
        }
        if !self.snapshot.borrow().should_confirm_on_clear {
            self.clear_history();
            return;
        }
        if self.is_clear_dialog_open.replace(true) {
            return;
        }

        let dialog = gtk::AlertDialog::builder()
            .modal(true)
            .message(translate_native("Clear all?"))
            .detail(format!(
                "{}\n{}",
                translate_native("Are you sure you want to delete all clipboard items?"),
                translate_native("This operation cannot be undone."),
            ))
            .buttons([translate_native("Cancel"), translate_native("Clear")])
            .cancel_button(0)
            .default_button(0)
            .build();
        let parent = self
            .popup
            .window()
            .is_visible()
            .then(|| self.popup.window().clone());
        let weak_controller = Rc::downgrade(self);
        let task = glib::spawn_future_local(async move {
            let response = dialog.choose_future(parent.as_ref()).await;
            let Some(controller) = weak_controller.upgrade() else {
                return;
            };
            controller.clear_dialog_task.borrow_mut().take();
            controller.is_clear_dialog_open.set(false);
            if response == Ok(1) && controller.is_running.get() && !controller.is_private_mode.get()
            {
                controller.clear_history();
            }
        });
        self.clear_dialog_task.replace(Some(task));
    }

    pub fn clear_history(self: &Rc<Self>) {
        self.close_clear_dialog();
        if !self.is_booted.get() {
            self.pending_clear.set(true);
            return;
        }
        let keep_selected = self.snapshot.borrow().should_keep_selected_on_clear;
        let result = self.store.borrow_mut().clear(keep_selected);
        self.delete_removed_images(result.removed);
        if result.should_clear_clipboard {
            self.clear_clipboard();
        }
        self.persist();
        self.refresh_popup_entries();
    }

    pub fn toggle_private_mode(self: &Rc<Self>) {
        let is_private_mode = !self.is_private_mode.get();
        self.is_private_mode.set(is_private_mode);
        self.clipboard.invalidate_pending();
        self.popup.set_private_mode(is_private_mode);
        if !is_private_mode {
            self.restore_selected_clipboard();
        }
    }

    fn restore_selected_clipboard(self: &Rc<Self>) {
        let selected = self.store.borrow().selected().cloned();
        let Some(bridge) = self.bridge.borrow().clone() else {
            return;
        };
        let clipboard = Rc::clone(&self.clipboard);
        let weak_controller = Rc::downgrade(self);
        self.spawn_auxiliary(async move {
            let result = match selected {
                Some(entry) => {
                    clipboard
                        .write(&bridge, &entry.mimetype, &entry.bytes)
                        .await
                }
                None => clipboard.clear(&bridge).await,
            };
            if let Err(error) = result {
                if let Some(controller) = weak_controller.upgrade() {
                    controller.report(&format!(
                        "Cannot restore the clipboard after private mode: {error}"
                    ));
                }
            }
        });
    }

    fn set_search_option(&self, name: &str, is_enabled: bool) {
        if let Err(error) = self.settings.set_search_option(name, is_enabled) {
            self.report(&format!("Cannot save the search option: {error}"));
        }
    }

    pub fn reset_clear_timer(self: &Rc<Self>) {
        if !self.is_booted.get() {
            self.pending_reset_timer.set(true);
            return;
        }
        self.scheduler.schedule();
    }

    fn on_settings_changed(self: &Rc<Self>) {
        sync_override_from_settings(self.settings.gio());
        let snapshot = self.settings.snapshot();
        let should_configure_shortcuts = {
            let previous = self.snapshot.borrow();
            previous.is_keybinding_enabled != snapshot.is_keybinding_enabled
                || previous.toggle_popup_shortcut != snapshot.toggle_popup_shortcut
                || previous.clear_history_shortcut != snapshot.clear_history_shortcut
                || previous.private_mode_shortcut != snapshot.private_mode_shortcut
        };
        {
            let previous = self.snapshot.borrow();
            if previous.should_cache_images != snapshot.should_cache_images
                || previous.excluded_apps != snapshot.excluded_apps
                || previous.should_ignore_secrets != snapshot.should_ignore_secrets
            {
                self.clipboard.invalidate_pending();
            }
        }
        *self.snapshot.borrow_mut() = snapshot.clone();
        if should_configure_shortcuts {
            self.configure_shortcuts();
        }
        self.popup.apply_settings(&snapshot);
        if !self.is_booted.get() {
            return;
        }
        let removed = {
            let mut store = self.store.borrow_mut();
            store.trim(snapshot.max_registry_length)
        };
        let removed_any = !removed.is_empty();
        self.delete_removed_images(removed);
        self.persist();
        if removed_any {
            self.refresh_popup_entries();
        }
    }

    fn close_clear_dialog(&self) {
        if let Some(task) = self.clear_dialog_task.borrow_mut().take() {
            task.abort();
        }
        self.is_clear_dialog_open.set(false);
    }

    fn persist(&self) {
        let should_cache_only_favorite = self.snapshot.borrow().should_cache_only_favorite;
        let entries = self
            .store
            .borrow()
            .to_persistable(should_cache_only_favorite);
        self.registry.write(entries);
    }

    fn refresh_popup_entries(&self) {
        let entries = self.store.borrow().entries().to_vec();
        self.popup.update_entries(entries);
    }

    fn clear_clipboard(self: &Rc<Self>) {
        let Some(bridge) = self.bridge.borrow().clone() else {
            return;
        };
        let clipboard = Rc::clone(&self.clipboard);
        let weak_controller = Rc::downgrade(self);
        self.spawn_auxiliary(async move {
            if let Err(error) = clipboard.clear(&bridge).await {
                if let Some(controller) = weak_controller.upgrade() {
                    controller.report(&format!("Cannot clear the clipboard: {error}"));
                }
            }
        });
    }

    fn delete_removed_images(self: &Rc<Self>, removed: Vec<RemovedEntry>) {
        for removed in removed {
            if removed.entry.is_image() {
                self.delete_entry_file(removed.entry);
            }
        }
    }

    fn delete_entry_file(self: &Rc<Self>, entry: ClipboardEntry) {
        let deletion = self.registry.delete_entry_file(&entry);
        let weak_controller = Rc::downgrade(self);
        self.spawn_auxiliary(async move {
            if let Err(error) = deletion.await {
                if let Some(controller) = weak_controller.upgrade() {
                    controller.report(&format!("Cannot delete a cached clipboard image: {error}"));
                }
            }
        });
    }

    fn spawn_auxiliary<F>(self: &Rc<Self>, future: F)
    where
        F: Future<Output = ()> + 'static,
    {
        let id = self.next_auxiliary_id.get().wrapping_add(1);
        self.next_auxiliary_id.set(id);
        let guard = AuxiliaryGuard {
            id,
            tasks: Rc::clone(&self.auxiliary_tasks),
        };
        let task = glib::spawn_future_local(async move {
            let _guard = guard;
            future.await;
        });
        self.auxiliary_tasks
            .borrow_mut()
            .push(AuxiliaryTask { id, task });
    }

    fn abort_auxiliary_tasks(&self) {
        let tasks: Vec<_> = self.auxiliary_tasks.borrow_mut().drain(..).collect();
        for tracked in tasks {
            tracked.task.abort();
        }
    }

    pub fn quit(self: &Rc<Self>) {
        if !self.is_running.get() {
            return;
        }
        self.is_running.set(false);
        self.should_present_settings.set(false);
        self.stop_shortcut_task();
        let settings_window = self.settings_window.borrow_mut().take();
        if let Some(window) = settings_window {
            window.destroy();
        }
        self.pending_activate.set(false);
        self.pending_clear.set(false);
        self.pending_reset_timer.set(false);
        self.opening_toggle.set(false);
        self.close_clear_dialog();
        self.scheduler.stop();
        self.clipboard.invalidate_pending();
        self.session.abort_boundary_task();
        self.abort_auxiliary_tasks();
        if let Some(task) = self.backend_task.borrow_mut().take() {
            task.abort();
        }
        if let Some(task) = self.boot_task.borrow_mut().take() {
            task.abort();
        }

        if matches!(self.session.phase(), Phase::Opening(_)) {
            self.session.cancel_opening();
        }
        let opening_task = self.session.take_opening_task();
        let token = self.session.active_token();
        let bridge = self.bridge.borrow().clone();
        self.popup.close();

        let controller = Rc::clone(self);
        let registry = Rc::clone(&self.registry);
        let application = self.application.upgrade();
        let _ = glib::spawn_future_local(async move {
            if let Some(bridge) = bridge.as_ref() {
                let _ = bridge.configure_shortcuts(&[]).await;
            }
            if let Some(opening_task) = opening_task {
                let _ = opening_task.await;
            }
            if let (Some(bridge), Some(token)) = (bridge, token) {
                let _ = bridge.cancel(token, true).await;
            }
            controller.session.reset();
            controller.target.replace(None);
            controller.bridge.take();
            if let Err(error) = registry.flush().await {
                eprintln!("WayToClip: cannot flush clipboard history: {error}");
            }
            if let Some(application) = application {
                application.quit();
            }
        });
    }

    pub fn shutdown(&self) {
        self.is_running.set(false);
        self.stop_shortcut_task();
        let settings_window = self.settings_window.borrow_mut().take();
        if let Some(window) = settings_window {
            window.destroy();
        }
        self.scheduler.stop();
        self.clipboard.invalidate_pending();
        self.session.abort_boundary_task();
        self.session.abort_opening_task();
        self.abort_auxiliary_tasks();
        if let Some(task) = self.backend_task.borrow_mut().take() {
            task.abort();
        }
        if let Some(task) = self.boot_task.borrow_mut().take() {
            task.abort();
        }
        self.close_clear_dialog();
        self.popup.close();
        self.popup.window().destroy();
        self.bridge.take();
        self.target.replace(None);
        self.session.reset();
        if let Some(handler) = self.settings_handler.borrow_mut().take() {
            self.settings.gio().disconnect(handler);
        }
    }

    fn report(&self, message: &str) {
        eprintln!("WayToClip: {message}");
        if let Some(application) = self.application.upgrade() {
            let notification = gio::Notification::new("WayToClip error");
            notification.set_body(Some(message));
            application.send_notification(Some("waytoclip-error"), &notification);
        }
    }
}
