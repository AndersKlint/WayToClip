use std::collections::HashMap;

use gtk::prelude::*;

use crate::common::i18n::translate_native;

use crate::settings::SettingsSnapshot;

use super::local_shortcut_utils::{
    self, Binding, DEFAULT_LOCAL_SHORTCUTS, KeyEvent, LocalActions, QUICK_SELECT_ORDER,
};
use super::popup_ui::PopupUi;

pub struct PopupLocalShortcuts {
    shortcut_strings: HashMap<&'static str, Vec<String>>,
    bindings: HashMap<&'static str, Vec<Binding>>,
    is_show_hints: bool,
    is_favorites_enabled: bool,
}

impl PopupLocalShortcuts {
    pub fn new() -> Self {
        let mut shortcuts = Self {
            shortcut_strings: HashMap::new(),
            bindings: HashMap::new(),
            is_show_hints: true,
            is_favorites_enabled: true,
        };
        for (action, defaults) in DEFAULT_LOCAL_SHORTCUTS.iter().copied() {
            let strings: Vec<String> = defaults.iter().map(|value| (*value).to_string()).collect();
            shortcuts.bindings.insert(
                action,
                local_shortcut_utils::parse_accelerator_list(defaults),
            );
            shortcuts.shortcut_strings.insert(action, strings);
        }
        shortcuts
    }

    pub fn apply_settings(&mut self, snapshot: &SettingsSnapshot) {
        for (action, defaults) in DEFAULT_LOCAL_SHORTCUTS.iter().copied() {
            let strings = snapshot
                .local_shortcuts
                .get(action)
                .cloned()
                .unwrap_or_else(|| defaults.iter().map(|value| (*value).to_string()).collect());
            let bindings = {
                let references: Vec<&str> = strings.iter().map(String::as_str).collect();
                local_shortcut_utils::parse_accelerator_list(&references)
            };
            self.shortcut_strings.insert(action, strings);
            self.bindings.insert(action, bindings);
        }
        self.is_show_hints = snapshot.should_show_shortcut_hints;
        self.is_favorites_enabled = snapshot.is_favorites_enabled;
    }

    pub fn matches(&self, action: &'static str, event: KeyEvent) -> bool {
        self.bindings
            .get(action)
            .is_some_and(|bindings| local_shortcut_utils::matches_shortcut(event, bindings))
    }

    pub fn is_bound(&self, action: &'static str) -> bool {
        self.bindings
            .get(action)
            .is_some_and(|bindings| !bindings.is_empty())
    }

    pub fn quick_select_slot(&self, event: KeyEvent) -> Option<usize> {
        QUICK_SELECT_ORDER
            .iter()
            .position(|action| self.matches(action, event))
    }

    pub fn quick_slot_display(&self, index: usize) -> Option<String> {
        let action = QUICK_SELECT_ORDER.get(index)?;
        if !self.is_bound(action) {
            return Some(String::new());
        }
        Some(self.format_for_hint(action))
    }

    pub fn format_for_hint(&self, action: &'static str) -> String {
        let Some(raw) = self
            .shortcut_strings
            .get(action)
            .and_then(|strings| strings.first())
            .filter(|raw| !raw.is_empty())
        else {
            return translate_native("Disabled");
        };
        local_shortcut_utils::format_accelerator(raw)
    }

    pub fn sync_hints(&self, ui: &PopupUi) {
        self.apply_hint(
            &ui.search_hint,
            &ui.search_hint_label,
            LocalActions::SEARCH,
            " = %s",
            "Toggle search (%s)",
        );
        self.apply_hint(
            &ui.private_hint,
            &ui.private_hint_label,
            LocalActions::PRIVATE_MODE,
            " = %s",
            "Toggle private mode (%s)",
        );
        if !self.is_favorites_enabled {
            ui.favorite_hint.set_visible(false);
        } else {
            self.apply_hint(
                &ui.favorite_hint,
                &ui.favorite_hint_label,
                LocalActions::TOGGLE_FAVORITE,
                " = %s",
                "Toggle favorite (%s)",
            );
        }
        self.apply_hint(
            &ui.delete_hint,
            &ui.delete_hint_label,
            LocalActions::DELETE_ENTRY,
            " = %s",
            "Delete selected entry (%s)",
        );
    }

    fn apply_hint(
        &self,
        hint: &gtk::Box,
        label: &gtk::Label,
        action: &'static str,
        label_format: &str,
        tooltip_format: &str,
    ) {
        let is_bound = self.is_bound(action);
        hint.set_visible(self.is_show_hints && is_bound);
        if !is_bound {
            return;
        }
        let display = self.format_for_hint(action);
        label.set_text(&label_format.replacen("%s", &display, 1));
        let tooltip = translate_native(tooltip_format).replacen("%s", &display, 1);
        hint.set_tooltip_text(Some(&tooltip));
    }
}
