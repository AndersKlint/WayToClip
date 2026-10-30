use crate::clipboard::clipboard_entry::ClipboardEntry;
use crate::settings::SettingsSnapshot;

use super::popup_search::PopupSearch;

pub struct PopupSearchController {
    model: PopupSearch,
    is_search_mode: bool,
    query: String,
}

impl PopupSearchController {
    pub fn new() -> Self {
        Self {
            model: PopupSearch::new(),
            is_search_mode: false,
            query: String::new(),
        }
    }

    pub fn apply_settings(&mut self, snapshot: &SettingsSnapshot) {
        self.model
            .apply_settings(snapshot.is_case_sensitive_search, snapshot.is_regex_search);
    }

    pub fn reset(&mut self) {
        self.is_search_mode = false;
        self.query.clear();
    }

    pub fn model(&self) -> &PopupSearch {
        &self.model
    }

    pub fn is_search_mode(&self) -> bool {
        self.is_search_mode
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn enter(&mut self) {
        self.is_search_mode = true;
    }

    pub fn exit(&mut self) {
        self.is_search_mode = false;
        self.query.clear();
    }

    pub fn set_query(&mut self, query: &str) {
        self.query.clear();
        self.query.push_str(query);
    }

    pub fn toggle_case_sensitive(&mut self) -> bool {
        let is_case_sensitive = !self.model.case_sensitive();
        self.model.set_case_sensitive(is_case_sensitive);
        is_case_sensitive
    }

    pub fn toggle_regex(&mut self) -> bool {
        let is_regex = !self.model.regex_enabled();
        self.model.set_regex_enabled(is_regex);
        is_regex
    }

    pub fn set_case_sensitive(&mut self, is_case_sensitive: bool) {
        self.model.set_case_sensitive(is_case_sensitive);
    }

    pub fn set_regex_enabled(&mut self, is_regex: bool) {
        self.model.set_regex_enabled(is_regex);
    }

    pub fn filter(&self, source: &[ClipboardEntry]) -> Vec<ClipboardEntry> {
        self.model.filter(source, &self.query)
    }
}
