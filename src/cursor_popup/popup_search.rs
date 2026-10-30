use regress::Regex;

use crate::clipboard::clipboard_entry::ClipboardEntry;

#[derive(Clone, Debug)]
pub struct PopupSearch {
    case_sensitive: bool,
    regex_enabled: bool,
}

impl Default for PopupSearch {
    fn default() -> Self {
        Self {
            case_sensitive: false,
            regex_enabled: false,
        }
    }
}

impl PopupSearch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply_settings(&mut self, case_sensitive: bool, regex_enabled: bool) {
        self.case_sensitive = case_sensitive;
        self.regex_enabled = regex_enabled;
    }

    pub fn case_sensitive(&self) -> bool {
        self.case_sensitive
    }

    pub fn regex_enabled(&self) -> bool {
        self.regex_enabled
    }

    pub fn set_case_sensitive(&mut self, case_sensitive: bool) {
        self.case_sensitive = case_sensitive;
    }

    pub fn set_regex_enabled(&mut self, regex_enabled: bool) {
        self.regex_enabled = regex_enabled;
    }

    pub fn filter(&self, items: &[ClipboardEntry], query: &str) -> Vec<ClipboardEntry> {
        if query.is_empty() {
            return items.to_vec();
        }

        items
            .iter()
            .filter(|entry| {
                let text = entry.get_string_value();
                if self.regex_enabled {
                    let flags = if self.case_sensitive { "" } else { "i" };
                    match Regex::with_flags(query, flags) {
                        Ok(regex) => regex.find(&text).is_some(),
                        Err(_) => contains(&text, query, self.case_sensitive),
                    }
                } else {
                    contains(&text, query, self.case_sensitive)
                }
            })
            .cloned()
            .collect()
    }
}

fn contains(text: &str, query: &str, case_sensitive: bool) -> bool {
    if case_sensitive {
        text.contains(query)
    } else {
        text.to_lowercase().contains(&query.to_lowercase())
    }
}
