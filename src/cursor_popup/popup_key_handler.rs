use gtk::{gdk, glib::translate::IntoGlib};

use crate::clipboard::clipboard_entry::ClipboardEntry;

use super::local_shortcut_utils::{KeyEvent, LocalActions};
use super::popup_local_shortcuts::PopupLocalShortcuts;
use super::popup_search_controller::PopupSearchController;
use super::popup_selection_controller::PopupSelectionController;

pub enum KeyOutcome {
    Propagate,
    Stop,
    Close {
        should_restore_focus: bool,
    },
    Select(ClipboardEntry),
    Remove(ClipboardEntry),
    ToggleFavorite(ClipboardEntry),
    TogglePrivateMode,
    SetSearchOption {
        name: &'static str,
        is_enabled: bool,
    },
    Render,
    RefreshSource,
    EnterSearch,
    ExitSearch,
}

pub struct PopupKeyHandler;

impl PopupKeyHandler {
    pub fn new() -> Self {
        Self
    }

    pub fn handle_key(
        &self,
        event: KeyEvent,
        search: &mut PopupSearchController,
        selection: &mut PopupSelectionController,
        shortcuts: &PopupLocalShortcuts,
    ) -> KeyOutcome {
        if search.is_search_mode() {
            return self.handle_search_key(event, search, selection, shortcuts);
        }
        self.handle_main_key(event, search, selection, shortcuts)
    }

    fn handle_main_key(
        &self,
        event: KeyEvent,
        search: &mut PopupSearchController,
        selection: &mut PopupSelectionController,
        shortcuts: &PopupLocalShortcuts,
    ) -> KeyOutcome {
        if shortcuts.matches(LocalActions::CLOSE, event) {
            return KeyOutcome::Close {
                should_restore_focus: true,
            };
        }

        if shortcuts.matches(LocalActions::CONFIRM, event) {
            let was_favorites_row = selection.is_favorites_row_selected();
            if let Some(entry) = selection.confirm_selection() {
                return KeyOutcome::Select(entry);
            }
            return if was_favorites_row {
                KeyOutcome::RefreshSource
            } else {
                KeyOutcome::Stop
            };
        }

        if shortcuts.matches(LocalActions::SEARCH, event) {
            search.enter();
            return KeyOutcome::EnterSearch;
        }

        if shortcuts.matches(LocalActions::DELETE_ENTRY, event) {
            return selection
                .delete_selected_item()
                .map_or(KeyOutcome::Stop, KeyOutcome::Remove);
        }

        if shortcuts.matches(LocalActions::PRIVATE_MODE, event) {
            return KeyOutcome::TogglePrivateMode;
        }

        if shortcuts.matches(LocalActions::TOGGLE_FAVORITE, event) {
            return selection
                .toggle_favorite_selected()
                .map_or(KeyOutcome::Stop, KeyOutcome::ToggleFavorite);
        }

        if shortcuts.matches(LocalActions::FAVORITES_VIEW, event) {
            selection.toggle_favorites_view();
            return KeyOutcome::RefreshSource;
        }

        if shortcuts.matches(LocalActions::PAGE_NEXT, event) {
            selection.navigate_page_forward();
            return KeyOutcome::Render;
        }

        if shortcuts.matches(LocalActions::PAGE_PREVIOUS, event) {
            selection.navigate_page_back();
            return KeyOutcome::Render;
        }

        if shortcuts.matches(LocalActions::MOVE_UP, event) {
            selection.navigate_up();
            return KeyOutcome::Render;
        }

        if shortcuts.matches(LocalActions::MOVE_DOWN, event) {
            selection.navigate_down();
            return KeyOutcome::Render;
        }

        if let Some(slot) = shortcuts.quick_select_slot(event) {
            return selection
                .select_by_slot_index(slot)
                .map_or(KeyOutcome::Stop, KeyOutcome::Select);
        }

        if event.keyval == gdk::Key::BackSpace.into_glib() {
            return KeyOutcome::Close {
                should_restore_focus: true,
            };
        }

        KeyOutcome::Propagate
    }

    fn handle_search_key(
        &self,
        event: KeyEvent,
        search: &mut PopupSearchController,
        selection: &mut PopupSelectionController,
        shortcuts: &PopupLocalShortcuts,
    ) -> KeyOutcome {
        if shortcuts.matches(LocalActions::CASE_SENSITIVE, event) {
            let is_enabled = search.toggle_case_sensitive();
            return KeyOutcome::SetSearchOption {
                name: "caseSensitive",
                is_enabled,
            };
        }

        if shortcuts.matches(LocalActions::REGEX, event) {
            let is_enabled = search.toggle_regex();
            return KeyOutcome::SetSearchOption {
                name: "regex",
                is_enabled,
            };
        }

        if shortcuts.matches(LocalActions::CONFIRM, event) {
            let was_favorites_row = selection.is_favorites_row_selected();
            return if let Some(entry) = selection.confirm_selection() {
                KeyOutcome::Select(entry)
            } else if was_favorites_row {
                KeyOutcome::RefreshSource
            } else {
                KeyOutcome::Stop
            };
        }

        if shortcuts.matches(LocalActions::CLOSE, event) {
            search.exit();
            return KeyOutcome::ExitSearch;
        }

        if shortcuts.matches(LocalActions::MOVE_UP, event) {
            selection.navigate_up();
            return KeyOutcome::Render;
        }

        if shortcuts.matches(LocalActions::MOVE_DOWN, event) {
            selection.navigate_down();
            return KeyOutcome::Render;
        }

        KeyOutcome::Propagate
    }
}
