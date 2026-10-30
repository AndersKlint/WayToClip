use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use glib::translate::IntoGlib;
use gtk::{gdk, glib, prelude::*};

use crate::clipboard::clipboard_entry::ClipboardEntry;
use crate::common::constants::ITEMS_PER_PAGE;
use crate::common::i18n::translate_native;
use crate::settings::SettingsSnapshot;

use super::image_preview::ImagePreview;
use super::local_shortcut_utils::{KeyEvent, LocalActions, ModMask};
use super::popup_key_handler::{KeyOutcome, PopupKeyHandler};
use super::popup_layout_placer::PopupLayoutPlacer;
use super::popup_local_shortcuts::PopupLocalShortcuts;
use super::popup_search_controller::PopupSearchController;
use super::popup_selection_controller::PopupSelectionController;
use super::popup_ui::PopupUi;

const DEFAULT_IMAGE_PREVIEW_SIZE: i32 = 96;

#[derive(Clone, Debug)]
pub enum PopupAction {
    SelectEntry(ClipboardEntry),
    RemoveEntry(ClipboardEntry),
    ToggleFavorite(ClipboardEntry),
    TogglePrivateMode,
    SetSearchOption { name: String, is_enabled: bool },
    Close { should_restore_focus: bool },
}

pub struct CursorPopup {
    window: gtk::ApplicationWindow,
    ui: PopupUi,
    action_handler: RefCell<Option<Rc<dyn Fn(PopupAction)>>>,
    selection: RefCell<PopupSelectionController>,
    search: RefCell<PopupSearchController>,
    shortcuts: RefCell<PopupLocalShortcuts>,
    layout: RefCell<PopupLayoutPlacer>,
    key_handler: PopupKeyHandler,
    image_preview_size: Cell<i32>,
    image_preview: RefCell<Option<ImagePreview>>,
    is_private_mode: Cell<bool>,
    is_open: Cell<bool>,
    has_activated: Cell<bool>,
    is_closing: Cell<bool>,
    is_updating_search: Cell<bool>,
    is_busy: Cell<bool>,
    self_weak: RefCell<Weak<Self>>,
    focus_loss_source: RefCell<Option<glib::SourceId>>,
}

impl CursorPopup {
    pub fn new(application: &gtk::Application) -> Rc<Self> {
        install_style();

        let window = gtk::ApplicationWindow::builder()
            .application(application)
            .title("WayToClip")
            .decorated(false)
            .resizable(false)
            .build();
        window.add_css_class("waytoclip-popup-window");

        let ui = PopupUi::new();
        window.set_child(Some(&ui.root));

        let popup = Rc::new(Self {
            window,
            ui,
            action_handler: RefCell::new(None),
            selection: RefCell::new(PopupSelectionController::new()),
            search: RefCell::new(PopupSearchController::new()),
            shortcuts: RefCell::new(PopupLocalShortcuts::new()),
            layout: RefCell::new(PopupLayoutPlacer::new()),
            key_handler: PopupKeyHandler::new(),
            image_preview_size: Cell::new(DEFAULT_IMAGE_PREVIEW_SIZE),
            image_preview: RefCell::new(None),
            is_private_mode: Cell::new(false),
            is_open: Cell::new(false),
            has_activated: Cell::new(false),
            is_closing: Cell::new(false),
            is_updating_search: Cell::new(false),
            is_busy: Cell::new(false),
            self_weak: RefCell::new(Weak::new()),
            focus_loss_source: RefCell::new(None),
        });
        popup.self_weak.replace(Rc::downgrade(&popup));
        popup.connect_signals();
        popup
    }

    pub fn set_action_handler(&self, handler: Rc<dyn Fn(PopupAction)>) {
        self.action_handler.replace(Some(handler));
    }

    pub fn window(&self) -> &gtk::ApplicationWindow {
        &self.window
    }

    pub fn prepare(
        self: &Rc<Self>,
        entries: Vec<ClipboardEntry>,
        snapshot: &SettingsSnapshot,
        available_width: i32,
        available_height: i32,
        is_private_mode: bool,
    ) {
        if self.is_open.get() {
            self.close();
        }
        self.cancel_focus_loss();

        self.apply_settings(snapshot);
        self.selection.borrow_mut().reset(&entries);
        self.search.borrow_mut().reset();
        self.is_private_mode.set(is_private_mode);
        self.is_open.set(true);
        self.has_activated.set(false);
        self.is_closing.set(false);

        let weak_popup = Rc::downgrade(self);
        self.image_preview.replace(Some(ImagePreview::new(move || {
            if let Some(popup) = weak_popup.upgrade() {
                popup.render();
            }
        })));
        self.set_search_text("");
        self.ui.set_search_visible(false);
        self.sync_search_ui();
        self.ui.set_private_hint_active(is_private_mode);
        self.layout
            .borrow_mut()
            .set_available_size(available_width, available_height);

        self.render();
    }

    pub fn natural_size(&self) -> (i32, i32) {
        self.layout.borrow().natural_size()
    }

    pub fn constrain_height(&self, height: i32) {
        self.layout.borrow_mut().set_available_height(height);
        self.render();
    }

    pub fn present(&self) {
        let (width, height) = self.natural_size();
        self.window.set_default_size(width.max(1), height.max(1));
        self.window.present();
        self.ui.root.grab_focus();
    }

    pub fn close(&self) {
        self.image_preview.borrow_mut().take();
        self.cancel_focus_loss();
        self.is_closing.set(true);
        self.is_open.set(false);
        self.has_activated.set(false);
        self.window.set_visible(false);

        self.set_search_text("");
        self.ui.set_search_visible(false);
        self.ui.set_status("");
        self.search.borrow_mut().reset();
        self.selection.borrow_mut().reset(&[]);
        self.layout.borrow_mut().clear();
        self.clear_list();
        self.ui.set_page_indicator(0, 1, false);
        self.ui.set_favorite_hint_active(false);
        self.is_closing.set(false);
    }

    pub fn set_busy(&self, is_busy: bool) {
        self.is_busy.set(is_busy);
        self.ui.list.set_sensitive(!is_busy);
        self.ui.search_bar.set_sensitive(!is_busy);
    }

    pub fn set_status(&self, message: &str) {
        self.ui.set_status(message);
        if self.is_open.get() {
            self.render();
        }
    }

    pub fn set_private_mode(&self, is_private_mode: bool) {
        self.is_private_mode.set(is_private_mode);
        self.ui.set_private_hint_active(is_private_mode);
    }

    pub fn update_entries(&self, entries: Vec<ClipboardEntry>) {
        let selected_entry = self.selection.borrow().get_selected_entry().cloned();
        let was_favorites_view = self.selection.borrow().is_favorites_view();
        let is_search_mode = self.search.borrow().is_search_mode();
        let has_query = !self.search.borrow().query().is_empty();

        {
            let mut selection = self.selection.borrow_mut();
            selection.reset(&entries);
            if was_favorites_view && selection.favorites_enabled() {
                selection.toggle_favorites_view();
            }
        }

        if !self.is_open.get() {
            return;
        }

        if is_search_mode && has_query {
            self.apply_filter();
        } else {
            let source = self.selection.borrow().get_search_source();
            self.selection.borrow_mut().show_filtered(&source);
        }
        if let Some(entry) = selected_entry {
            self.restore_selected_entry(&entry);
        }
        self.render();
    }

    pub fn apply_settings(&self, snapshot: &SettingsSnapshot) {
        self.image_preview_size
            .set(snapshot.image_preview_size.clamp(32, 512));
        self.shortcuts.borrow_mut().apply_settings(snapshot);
        self.search.borrow_mut().apply_settings(snapshot);

        let was_favorites_view = self.selection.borrow().is_favorites_view();
        self.selection.borrow_mut().apply_settings(
            snapshot.should_limit_popup_pages,
            snapshot.max_popup_pages,
            snapshot.is_favorites_enabled,
        );
        if was_favorites_view && !snapshot.is_favorites_enabled {
            let source = self.selection.borrow().get_search_source();
            self.selection.borrow_mut().show_filtered(&source);
        }

        self.sync_search_ui();
        self.sync_hints();
        if !self.is_open.get() {
            return;
        }
        let should_refilter = {
            let search = self.search.borrow();
            search.is_search_mode() && !search.query().is_empty()
        };
        if should_refilter {
            self.apply_filter_and_render();
        } else {
            self.render();
        }
    }

    fn connect_signals(self: &Rc<Self>) {
        let weak_popup = Rc::downgrade(self);
        self.window.connect_scale_factor_notify(move |_| {
            if let Some(popup) = weak_popup.upgrade() {
                popup.render();
            }
        });
        let weak_popup = Rc::downgrade(self);
        self.ui.search_entry.connect_changed(move |entry| {
            let Some(popup) = weak_popup.upgrade() else {
                return;
            };
            if popup.is_updating_search.get() || popup.is_busy.get() {
                return;
            }
            popup.search.borrow_mut().set_query(&entry.text());
            if popup.search.borrow().is_search_mode() {
                popup.apply_filter_and_render();
            }
        });

        let weak_bar = self.ui.search_bar.downgrade();
        let focus = gtk::EventControllerFocus::new();
        focus.connect_enter(move |_| {
            if let Some(bar) = weak_bar.upgrade() {
                bar.add_css_class("focus");
            }
        });
        let weak_bar = self.ui.search_bar.downgrade();
        focus.connect_leave(move |_| {
            if let Some(bar) = weak_bar.upgrade() {
                bar.remove_css_class("focus");
            }
        });
        self.ui.search_entry.add_controller(focus);

        let weak_popup = Rc::downgrade(self);
        self.ui.case_button.connect_clicked(move |button| {
            if let Some(popup) = weak_popup.upgrade() {
                if !popup.is_busy.get() {
                    popup.apply_search_option("caseSensitive", button.is_active());
                }
            }
        });
        let weak_popup = Rc::downgrade(self);
        self.ui.regex_button.connect_clicked(move |button| {
            if let Some(popup) = weak_popup.upgrade() {
                if !popup.is_busy.get() {
                    popup.apply_search_option("regex", button.is_active());
                }
            }
        });

        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak_popup = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(popup) = weak_popup.upgrade() else {
                return glib::Propagation::Proceed;
            };
            popup.handle_key_event(KeyEvent {
                keyval: key.into_glib(),
                state: normalize_key_state(state),
            })
        });
        self.window.add_controller(keys);

        let weak_popup = Rc::downgrade(self);
        self.window.connect_is_active_notify(move |window| {
            let Some(popup) = weak_popup.upgrade() else {
                return;
            };
            if !popup.is_open.get() {
                return;
            }
            if window.is_active() {
                popup.has_activated.set(true);
                popup.cancel_focus_loss();
                return;
            }
            if !popup.has_activated.get() || popup.is_closing.get() || !window.is_visible() {
                return;
            }
            popup.has_activated.set(false);
            popup.cancel_focus_loss();
            let weak_popup = popup.self_weak.borrow().clone();
            let source = glib::idle_add_local_once(move || {
                pending_focus_loss(&weak_popup);
            });
            popup.focus_loss_source.replace(Some(source));
        });

        let weak_popup = Rc::downgrade(self);
        self.window.connect_close_request(move |_| {
            let Some(popup) = weak_popup.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if popup.is_open.get() && !popup.is_closing.get() {
                popup.cancel_focus_loss();
                popup.has_activated.set(false);
                popup.emit_action(PopupAction::Close {
                    should_restore_focus: true,
                });
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }

    fn handle_key_event(&self, event: KeyEvent) -> glib::Propagation {
        if self.is_busy.get() {
            let should_close = {
                let shortcuts = self.shortcuts.borrow();
                shortcuts.matches(LocalActions::CLOSE, event)
                    || (event.keyval == gdk::Key::BackSpace.into_glib()
                        && !self.search.borrow().is_search_mode())
            };
            if should_close {
                self.emit_action(PopupAction::Close {
                    should_restore_focus: true,
                });
            }
            return glib::Propagation::Stop;
        }

        let outcome = {
            let mut search = self.search.borrow_mut();
            let mut selection = self.selection.borrow_mut();
            let shortcuts = self.shortcuts.borrow();
            self.key_handler
                .handle_key(event, &mut search, &mut selection, &shortcuts)
        };

        match outcome {
            KeyOutcome::Propagate => glib::Propagation::Proceed,
            KeyOutcome::Stop => glib::Propagation::Stop,
            KeyOutcome::Close {
                should_restore_focus,
            } => {
                self.emit_action(PopupAction::Close {
                    should_restore_focus,
                });
                glib::Propagation::Stop
            }
            KeyOutcome::Select(entry) => {
                self.emit_action(PopupAction::SelectEntry(entry));
                glib::Propagation::Stop
            }
            KeyOutcome::Remove(entry) => {
                self.emit_action(PopupAction::RemoveEntry(entry));
                self.render();
                glib::Propagation::Stop
            }
            KeyOutcome::ToggleFavorite(entry) => {
                self.emit_action(PopupAction::ToggleFavorite(entry));
                self.render();
                glib::Propagation::Stop
            }
            KeyOutcome::TogglePrivateMode => {
                self.emit_action(PopupAction::TogglePrivateMode);
                glib::Propagation::Stop
            }
            KeyOutcome::SetSearchOption { name, is_enabled } => {
                self.apply_search_option(name, is_enabled);
                glib::Propagation::Stop
            }
            KeyOutcome::Render => {
                self.render();
                glib::Propagation::Stop
            }
            KeyOutcome::RefreshSource => {
                self.refresh_source_and_render();
                glib::Propagation::Stop
            }
            KeyOutcome::EnterSearch => {
                self.enter_search();
                glib::Propagation::Stop
            }
            KeyOutcome::ExitSearch => {
                self.exit_search();
                glib::Propagation::Stop
            }
        }
    }

    fn apply_search_option(&self, name: &'static str, is_enabled: bool) {
        if self.is_busy.get() {
            return;
        }
        {
            let mut search = self.search.borrow_mut();
            match name {
                "caseSensitive" => search.set_case_sensitive(is_enabled),
                "regex" => search.set_regex_enabled(is_enabled),
                _ => return,
            }
        }
        self.sync_search_ui();
        self.emit_action(PopupAction::SetSearchOption {
            name: name.to_string(),
            is_enabled,
        });
        if self.search.borrow().is_search_mode() {
            self.apply_filter_and_render();
        }
    }

    fn enter_search(&self) {
        self.search.borrow_mut().enter();
        self.ui.set_search_visible(true);
        self.sync_search_ui();
        self.ui.search_entry.grab_focus();
        self.render();
    }

    fn exit_search(&self) {
        self.search.borrow_mut().exit();
        self.set_search_text("");
        self.ui.set_search_visible(false);
        self.sync_search_ui();
        self.apply_filter_and_render();
        self.ui.root.grab_focus();
    }

    fn apply_filter(&self) {
        let source = self.selection.borrow().get_search_source();
        let filtered = self.search.borrow().filter(&source);
        self.selection.borrow_mut().show_filtered(&filtered);
    }

    fn apply_filter_and_render(&self) {
        self.apply_filter();
        self.render();
    }

    fn refresh_source_and_render(&self) {
        if self.search.borrow().is_search_mode() && !self.search.borrow().query().is_empty() {
            self.apply_filter_and_render();
            return;
        }
        let source = self.selection.borrow().get_search_source();
        self.selection.borrow_mut().show_filtered(&source);
        self.render();
    }

    fn restore_selected_entry(&self, target: &ClipboardEntry) {
        let (page, slot) = {
            let selection = self.selection.borrow();
            let Some(index) = selection
                .items_to_show()
                .iter()
                .position(|entry| entry.equals(target))
            else {
                return;
            };
            (index / ITEMS_PER_PAGE, index % ITEMS_PER_PAGE)
        };
        let mut selection = self.selection.borrow_mut();
        for _ in 0..page {
            selection.navigate_page_forward();
        }
        for _ in 0..slot {
            selection.navigate_down();
        }
    }

    fn render(&self) {
        if !self.is_open.get() {
            return;
        }

        let weak_popup = self.self_weak.borrow().clone();
        let on_select: Rc<dyn Fn(ClipboardEntry)> = Rc::new(move |entry| {
            if let Some(popup) = weak_popup.upgrade() {
                if !popup.is_busy.get() {
                    popup.emit_action(PopupAction::SelectEntry(entry));
                }
            }
        });
        let weak_popup = self.self_weak.borrow().clone();
        let on_favorites: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(popup) = weak_popup.upgrade() {
                if !popup.is_busy.get() {
                    popup.selection.borrow_mut().toggle_favorites_view();
                    popup.refresh_source_and_render();
                }
            }
        });

        let (width, height) = {
            let image_preview = self.image_preview.borrow();
            let Some(image_preview) = image_preview.as_ref() else {
                return;
            };
            let layout = self.layout.borrow();
            let selection = self.selection.borrow();
            let search = self.search.borrow();
            let shortcuts = self.shortcuts.borrow();
            layout.render(
                &self.ui,
                &selection,
                &search,
                &shortcuts,
                self.image_preview_size.get(),
                image_preview,
                &on_select,
                &on_favorites,
            );
            layout.natural_size()
        };
        self.window.set_default_size(width.max(1), height.max(1));
    }

    fn sync_search_ui(&self) {
        let (is_case_sensitive, is_regex) = {
            let search = self.search.borrow();
            (
                search.model().case_sensitive(),
                search.model().regex_enabled(),
            )
        };
        self.ui
            .set_search_toggle_state(&self.ui.case_button, is_case_sensitive);
        self.ui
            .set_search_toggle_state(&self.ui.regex_button, is_regex);

        let (case_hint, regex_hint) = {
            let shortcuts = self.shortcuts.borrow();
            (
                shortcuts.format_for_hint(LocalActions::CASE_SENSITIVE),
                shortcuts.format_for_hint(LocalActions::REGEX),
            )
        };
        let case_tooltip = translate_native("Match Case (%s)").replacen("%s", &case_hint, 1);
        let regex_tooltip =
            translate_native("Use Regular Expression (%s)").replacen("%s", &regex_hint, 1);
        self.ui.case_button.set_tooltip_text(Some(&case_tooltip));
        self.ui.regex_button.set_tooltip_text(Some(&regex_tooltip));
    }

    fn sync_hints(&self) {
        self.shortcuts.borrow().sync_hints(&self.ui);
    }

    fn set_search_text(&self, text: &str) {
        self.is_updating_search.set(true);
        self.ui.search_entry.set_text(text);
        self.is_updating_search.set(false);
    }

    fn clear_list(&self) {
        while let Some(child) = self.ui.list.first_child() {
            self.ui.list.remove(&child);
        }
    }

    fn emit_action(&self, action: PopupAction) {
        let handler = self.action_handler.borrow().clone();
        if let Some(handler) = handler {
            handler(action);
        }
    }

    fn cancel_focus_loss(&self) {
        if let Some(source) = self.focus_loss_source.borrow_mut().take() {
            source.remove();
        }
    }
}

impl Drop for CursorPopup {
    fn drop(&mut self) {
        if let Some(source) = self.focus_loss_source.get_mut().take() {
            source.remove();
        }
        self.layout.get_mut().clear();
        self.window.destroy();
    }
}

fn normalize_key_state(state: gdk::ModifierType) -> u32 {
    let mut modifiers = 0;
    if state.contains(gdk::ModifierType::SHIFT_MASK) {
        modifiers |= ModMask::SHIFT;
    }
    if state.contains(gdk::ModifierType::CONTROL_MASK) {
        modifiers |= ModMask::CONTROL;
    }
    if state.contains(gdk::ModifierType::ALT_MASK) {
        modifiers |= ModMask::MOD1;
    }
    if state.intersects(gdk::ModifierType::SUPER_MASK | gdk::ModifierType::META_MASK)
        || state.bits() & (1 << 6) != 0
    {
        modifiers |= ModMask::MOD4;
    }
    modifiers
}

fn pending_focus_loss(weak_popup: &Weak<CursorPopup>) {
    let Some(popup) = weak_popup.upgrade() else {
        return;
    };
    popup.focus_loss_source.borrow_mut().take();
    if popup.is_open.get() && !popup.is_closing.get() && popup.window.is_visible() {
        popup.emit_action(PopupAction::Close {
            should_restore_focus: false,
        });
    }
}

fn install_style() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_string(include_str!("style.css"));
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
