use std::sync::{Mutex, OnceLock};

use gtk::gio::prelude::*;
use gtk::{gio, glib};
use serde_json::Value;

use crate::common::constants::PrefsFields;

pub const SYSTEM_LANGUAGE: &str = "system";
pub const GETTEXT_DOMAIN: &str = "waytoclip";
pub const HAS_BUNDLED_CATALOGS: bool = true;

static TRANSLATIONS: OnceLock<Value> = OnceLock::new();

pub const LANGUAGE_LABELS: &[(&str, &str)] = &[
    ("ar", "العربية"),
    ("ca", "Català"),
    ("cs", "Čeština"),
    ("de", "Deutsch"),
    ("el", "Ελληνικά"),
    ("es", "Español"),
    ("eu", "Euskara"),
    ("fa", "فارسی"),
    ("fi", "Suomi"),
    ("fr_FR", "Français"),
    ("hu", "Magyar"),
    ("it", "Italiano"),
    ("ja", "日本語"),
    ("kn", "ಕನ್ನಡ"),
    ("ko", "한국어"),
    ("nl", "Nederlands"),
    ("oc", "Occitan"),
    ("pl", "Polski"),
    ("pt_BR", "Português (Brasil)"),
    ("ru", "Русский"),
    ("sk", "Slovenčina"),
    ("tr", "Türkçe"),
    ("uk", "Українська"),
    ("zh_CN", "简体中文"),
    ("zh_TW", "繁體中文（臺灣）"),
];

static LANGUAGE_OVERRIDE: OnceLock<Mutex<String>> = OnceLock::new();

fn override_cell() -> &'static Mutex<String> {
    LANGUAGE_OVERRIDE.get_or_init(|| Mutex::new(SYSTEM_LANGUAGE.to_string()))
}

pub fn available_languages() -> Vec<&'static str> {
    let mut codes: Vec<&'static str> = LANGUAGE_LABELS.iter().map(|(code, _)| *code).collect();
    codes.sort_by_key(|code| code.to_ascii_lowercase());
    codes.insert(0, SYSTEM_LANGUAGE);
    codes
}

pub fn language_label(code: &str) -> Option<&'static str> {
    if code == SYSTEM_LANGUAGE {
        return None;
    }
    LANGUAGE_LABELS
        .iter()
        .find_map(|(known, label)| (*known == code).then_some(*label))
}

pub fn is_known_language(code: &str) -> bool {
    code == SYSTEM_LANGUAGE || LANGUAGE_LABELS.iter().any(|(known, _)| *known == code)
}

pub fn normalize_language(code: &str) -> String {
    if code.is_empty() {
        return SYSTEM_LANGUAGE.to_string();
    }
    if code == SYSTEM_LANGUAGE || is_known_language(code) {
        return code.to_string();
    }
    let dashed = code.replace('-', "_");
    if is_known_language(&dashed) {
        return dashed;
    }
    SYSTEM_LANGUAGE.to_string()
}

pub fn set_language_override(value: &str) {
    let mut override_value = override_cell()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *override_value = normalize_language(value);
}

pub fn get_language_override() -> String {
    override_cell()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

pub fn reset_language_override() {
    set_language_override(SYSTEM_LANGUAGE);
}

pub fn get_stored_language(settings: &gio::Settings) -> String {
    normalize_language(&settings.string(PrefsFields::LANGUAGE))
}

pub fn sync_override_from_settings(settings: &gio::Settings) {
    set_language_override(&get_stored_language(settings));
}

pub fn native_gettext(msgid: &str) -> String {
    glib::dgettext(Some(GETTEXT_DOMAIN), msgid).to_string()
}

pub fn translate<F>(msgid: &str, fallback: F) -> String
where
    F: FnOnce(&str) -> String,
{
    let language = get_language_override();
    if language == SYSTEM_LANGUAGE {
        let native = fallback(msgid);
        if native != msgid {
            return native;
        }
        return system_catalog_hit(msgid)
            .map(ToOwned::to_owned)
            .unwrap_or(native);
    }
    match catalog_hit(&language, msgid) {
        Some(text) => text.to_string(),
        None => fallback(msgid),
    }
}

fn catalog_hit(language: &str, msgid: &str) -> Option<&'static str> {
    translations()
        .get(language)
        .and_then(|catalog| catalog.get(msgid))
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
}

fn system_catalog_hit(msgid: &str) -> Option<&'static str> {
    system_language_candidates()
        .iter()
        .find_map(|language| catalog_hit(language, msgid))
}

fn system_language_candidates() -> Vec<String> {
    let mut candidates = Vec::new();
    for variable in ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"] {
        let Ok(value) = std::env::var(variable) else {
            continue;
        };
        for locale in value.split(':') {
            let base = locale.split('.').next().unwrap_or(locale);
            let base = base.split('@').next().unwrap_or(base);
            if base.is_empty() || base == "C" || base == "POSIX" {
                continue;
            }
            let normalized = base.replace('-', "_");
            let language = normalized.split('_').next().unwrap_or(&normalized);
            for candidate in [normalized.as_str(), language] {
                if !candidate.is_empty()
                    && !candidates.iter().any(|known: &String| known == candidate)
                {
                    candidates.push(candidate.to_string());
                }
            }
        }
    }
    candidates
}

fn translations() -> &'static Value {
    TRANSLATIONS.get_or_init(|| {
        serde_json::from_str(include_str!("translations.json")).unwrap_or(Value::Null)
    })
}

pub fn translate_native(msgid: &str) -> String {
    translate(msgid, native_gettext)
}
