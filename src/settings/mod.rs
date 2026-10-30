use std::collections::BTreeMap;

mod controls;
mod shortcut_editor;
mod shortcut_highlighter;
mod string_list_manager;
pub mod window;
pub use window::SettingsWindow;

use gtk::gio::prelude::*;
use gtk::{gio, glib};

use crate::common::{constants::PrefsFields, i18n};

pub const SCHEMA_ID: &str = "org.waytoclip.WayToClip";

const LOCAL_SCHEMA_DIR: &str = concat!(env!("OUT_DIR"), "/schemas");

const LOCAL_SHORTCUT_KEYS: &[(&str, &str)] = &[
    ("search", PrefsFields::LOCAL_SEARCH),
    ("deleteEntry", PrefsFields::LOCAL_DELETE_ENTRY),
    ("privateMode", PrefsFields::LOCAL_PRIVATE_MODE),
    ("toggleFavorite", PrefsFields::LOCAL_TOGGLE_FAVORITE),
    ("favoritesView", PrefsFields::LOCAL_FAVORITES_VIEW),
    ("pageNext", PrefsFields::LOCAL_PAGE_NEXT),
    ("pagePrevious", PrefsFields::LOCAL_PAGE_PREVIOUS),
    ("moveUp", PrefsFields::LOCAL_MOVE_UP),
    ("moveDown", PrefsFields::LOCAL_MOVE_DOWN),
    ("confirm", PrefsFields::LOCAL_CONFIRM),
    ("close", PrefsFields::LOCAL_CLOSE),
    ("caseSensitive", PrefsFields::LOCAL_CASE_SENSITIVE),
    ("regex", PrefsFields::LOCAL_REGEX_SEARCH),
    ("quickSelect1", PrefsFields::LOCAL_QUICK_SELECT_1),
    ("quickSelect2", PrefsFields::LOCAL_QUICK_SELECT_2),
    ("quickSelect3", PrefsFields::LOCAL_QUICK_SELECT_3),
    ("quickSelect4", PrefsFields::LOCAL_QUICK_SELECT_4),
    ("quickSelect5", PrefsFields::LOCAL_QUICK_SELECT_5),
    ("quickSelect6", PrefsFields::LOCAL_QUICK_SELECT_6),
    ("quickSelect7", PrefsFields::LOCAL_QUICK_SELECT_7),
    ("quickSelect8", PrefsFields::LOCAL_QUICK_SELECT_8),
    ("quickSelect9", PrefsFields::LOCAL_QUICK_SELECT_9),
    ("quickSelect10", PrefsFields::LOCAL_QUICK_SELECT_10),
];

#[derive(Clone, Debug)]
pub struct SettingsSnapshot {
    pub max_registry_length: usize,
    pub cache_file_size_mb: usize,
    pub should_cache_only_favorite: bool,
    pub should_move_item_first: bool,
    pub should_confirm_on_clear: bool,
    pub is_keybinding_enabled: bool,
    pub should_clear_on_boot: bool,
    pub should_keep_selected_on_clear: bool,
    pub should_cache_images: bool,
    pub excluded_apps: Vec<String>,
    pub terminal_apps: Vec<String>,
    pub should_clear_history_on_interval: bool,
    pub clear_history_interval: u32,
    pub next_history_clear: i64,
    pub popup_position_mode: i32,
    pub max_popup_pages: usize,
    pub should_limit_popup_pages: bool,
    pub should_auto_paste: bool,
    pub is_case_sensitive_search: bool,
    pub is_regex_search: bool,
    pub should_show_shortcut_hints: bool,
    pub should_ignore_secrets: bool,
    pub is_favorites_enabled: bool,
    pub local_shortcuts: BTreeMap<String, Vec<String>>,
    pub toggle_popup_shortcut: Vec<String>,
    pub clear_history_shortcut: Vec<String>,
    pub private_mode_shortcut: Vec<String>,
    pub image_preview_size: i32,
    pub language: String,
}

pub struct SettingsManager {
    settings: gio::Settings,
}

impl SettingsManager {
    pub fn new() -> Result<Self, glib::Error> {
        let schema = find_schema()?;
        let backend = gio::SettingsBackend::default();
        let settings = gio::Settings::new_full(&schema, Some(&backend), None);
        let manager = Self { settings };
        i18n::sync_override_from_settings(&manager.settings);
        manager
            .settings
            .connect_changed(Some(PrefsFields::LANGUAGE), |settings, _| {
                i18n::sync_override_from_settings(settings);
            });
        Ok(manager)
    }

    pub fn snapshot(&self) -> SettingsSnapshot {
        let settings = &self.settings;
        let mut local_shortcuts = BTreeMap::new();
        for (action, key) in LOCAL_SHORTCUT_KEYS {
            local_shortcuts.insert((*action).to_string(), read_strv(settings, key));
        }

        SettingsSnapshot {
            max_registry_length: settings.int(PrefsFields::HISTORY_SIZE) as usize,
            cache_file_size_mb: settings.int(PrefsFields::CACHE_FILE_SIZE) as usize,
            should_cache_only_favorite: settings.boolean(PrefsFields::CACHE_ONLY_FAVORITE),
            should_move_item_first: settings.boolean(PrefsFields::MOVE_ITEM_FIRST),
            should_confirm_on_clear: settings.boolean(PrefsFields::CONFIRM_ON_CLEAR),
            is_keybinding_enabled: settings.boolean(PrefsFields::ENABLE_KEYBINDING),
            should_clear_on_boot: settings.boolean(PrefsFields::CLEAR_ON_BOOT),
            should_keep_selected_on_clear: settings.boolean(PrefsFields::KEEP_SELECTED_ON_CLEAR),
            should_cache_images: settings.boolean(PrefsFields::SHOULD_CACHE_IMAGES),
            excluded_apps: read_strv(settings, PrefsFields::EXCLUDED_APPS),
            terminal_apps: read_strv(settings, PrefsFields::TERMINAL_APPS),
            should_clear_history_on_interval: settings
                .boolean(PrefsFields::CLEAR_HISTORY_ON_INTERVAL),
            clear_history_interval: settings.int(PrefsFields::CLEAR_HISTORY_INTERVAL) as u32,
            next_history_clear: settings.int(PrefsFields::NEXT_HISTORY_CLEAR) as i64,
            popup_position_mode: settings.int(PrefsFields::POPUP_POSITION_MODE),
            max_popup_pages: settings.int(PrefsFields::MAX_POPUP_PAGES) as usize,
            should_limit_popup_pages: settings.boolean(PrefsFields::LIMIT_POPUP_PAGES),
            should_auto_paste: settings.boolean(PrefsFields::AUTO_PASTE),
            is_case_sensitive_search: settings.boolean(PrefsFields::CASE_SENSITIVE_SEARCH),
            is_regex_search: settings.boolean(PrefsFields::REGEX_SEARCH),
            should_show_shortcut_hints: settings.boolean(PrefsFields::SHOW_SHORTCUT_HINTS),
            should_ignore_secrets: settings.boolean(PrefsFields::IGNORE_SECRET_MIMETYPES),
            is_favorites_enabled: settings.boolean(PrefsFields::ENABLE_FAVORITES),
            local_shortcuts,
            toggle_popup_shortcut: read_strv(settings, PrefsFields::BINDING_TOGGLE_POPUP),
            clear_history_shortcut: read_strv(settings, PrefsFields::BINDING_CLEAR_HISTORY),
            private_mode_shortcut: read_strv(settings, PrefsFields::BINDING_PRIVATE_MODE),
            image_preview_size: settings.int(PrefsFields::IMAGE_PREVIEW_SIZE),
            language: settings.string(PrefsFields::LANGUAGE).to_string(),
        }
    }

    pub fn gio(&self) -> &gio::Settings {
        &self.settings
    }

    pub fn set_search_option(&self, name: &str, is_enabled: bool) -> Result<(), glib::BoolError> {
        let key = match name {
            "caseSensitive" => PrefsFields::CASE_SENSITIVE_SEARCH,
            "regex" => PrefsFields::REGEX_SEARCH,
            _ => {
                return Err(glib::BoolError::new(
                    format!("unknown search option: {name}"),
                    file!(),
                    "set_search_option",
                    line!(),
                ));
            }
        };
        self.settings.set_boolean(key, is_enabled)
    }

    pub fn set_next_history_clear(&self, timestamp: i64) -> Result<(), glib::BoolError> {
        self.settings
            .set_int(PrefsFields::NEXT_HISTORY_CLEAR, timestamp as i32)
    }
}

fn find_schema() -> Result<gio::SettingsSchema, glib::Error> {
    if let Some(schema) =
        gio::SettingsSchemaSource::default().and_then(|source| source.lookup(SCHEMA_ID, true))
    {
        return Ok(schema);
    }
    let local = gio::SettingsSchemaSource::from_directory(LOCAL_SCHEMA_DIR, None, true)?;
    local.lookup(SCHEMA_ID, true).ok_or_else(|| {
        glib::Error::new(
            gio::IOErrorEnum::NotFound,
            "GSettings schema org.waytoclip.WayToClip is unavailable",
        )
    })
}

fn read_strv(settings: &gio::Settings, key: &str) -> Vec<String> {
    settings
        .strv(key)
        .iter()
        .map(|value| value.to_string())
        .collect()
}
