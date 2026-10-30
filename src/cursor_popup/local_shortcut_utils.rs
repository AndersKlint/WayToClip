use crate::common::constants::PrefsFields;

pub struct ModMask;

impl ModMask {
    pub const SHIFT: u32 = 1 << 0;
    pub const LOCK: u32 = 1 << 1;
    pub const CONTROL: u32 = 1 << 2;
    pub const MOD1: u32 = 1 << 3;
    pub const MOD2: u32 = 1 << 4;
    pub const MOD3: u32 = 1 << 5;
    pub const MOD4: u32 = 1 << 6;
    pub const MOD5: u32 = 1 << 7;
}

pub const SIGNIFICANT_MODS: u32 = ModMask::CONTROL | ModMask::SHIFT | ModMask::MOD1 | ModMask::MOD4;

pub struct LocalActions;

impl LocalActions {
    pub const SEARCH: &'static str = "search";
    pub const DELETE_ENTRY: &'static str = "deleteEntry";
    pub const PRIVATE_MODE: &'static str = "privateMode";
    pub const TOGGLE_FAVORITE: &'static str = "toggleFavorite";
    pub const FAVORITES_VIEW: &'static str = "favoritesView";
    pub const PAGE_NEXT: &'static str = "pageNext";
    pub const PAGE_PREVIOUS: &'static str = "pagePrevious";
    pub const MOVE_UP: &'static str = "moveUp";
    pub const MOVE_DOWN: &'static str = "moveDown";
    pub const CONFIRM: &'static str = "confirm";
    pub const CLOSE: &'static str = "close";
    pub const CASE_SENSITIVE: &'static str = "caseSensitive";
    pub const REGEX: &'static str = "regex";
    pub const QUICK_SELECT_1: &'static str = "quickSelect1";
    pub const QUICK_SELECT_2: &'static str = "quickSelect2";
    pub const QUICK_SELECT_3: &'static str = "quickSelect3";
    pub const QUICK_SELECT_4: &'static str = "quickSelect4";
    pub const QUICK_SELECT_5: &'static str = "quickSelect5";
    pub const QUICK_SELECT_6: &'static str = "quickSelect6";
    pub const QUICK_SELECT_7: &'static str = "quickSelect7";
    pub const QUICK_SELECT_8: &'static str = "quickSelect8";
    pub const QUICK_SELECT_9: &'static str = "quickSelect9";
    pub const QUICK_SELECT_10: &'static str = "quickSelect10";
}

pub const QUICK_SELECT_ORDER: [&str; 10] = [
    LocalActions::QUICK_SELECT_1,
    LocalActions::QUICK_SELECT_2,
    LocalActions::QUICK_SELECT_3,
    LocalActions::QUICK_SELECT_4,
    LocalActions::QUICK_SELECT_5,
    LocalActions::QUICK_SELECT_6,
    LocalActions::QUICK_SELECT_7,
    LocalActions::QUICK_SELECT_8,
    LocalActions::QUICK_SELECT_9,
    LocalActions::QUICK_SELECT_10,
];

pub const DEFAULT_LOCAL_SHORTCUTS: [(&str, &[&str]); 23] = [
    (LocalActions::SEARCH, &["s"]),
    (LocalActions::DELETE_ENTRY, &["d"]),
    (LocalActions::PRIVATE_MODE, &["p"]),
    (LocalActions::TOGGLE_FAVORITE, &["f"]),
    (LocalActions::FAVORITES_VIEW, &["-"]),
    (LocalActions::PAGE_NEXT, &["Tab", "Right"]),
    (
        LocalActions::PAGE_PREVIOUS,
        &["<Shift>ISO_Left_Tab", "Left"],
    ),
    (LocalActions::MOVE_UP, &["Up"]),
    (LocalActions::MOVE_DOWN, &["Down"]),
    (LocalActions::CONFIRM, &["Return", "KP_Enter"]),
    (LocalActions::CLOSE, &["Escape"]),
    (LocalActions::CASE_SENSITIVE, &["<Alt>c"]),
    (LocalActions::REGEX, &["<Alt>r"]),
    (LocalActions::QUICK_SELECT_1, &["1"]),
    (LocalActions::QUICK_SELECT_2, &["2"]),
    (LocalActions::QUICK_SELECT_3, &["3"]),
    (LocalActions::QUICK_SELECT_4, &["4"]),
    (LocalActions::QUICK_SELECT_5, &["5"]),
    (LocalActions::QUICK_SELECT_6, &["6"]),
    (LocalActions::QUICK_SELECT_7, &["7"]),
    (LocalActions::QUICK_SELECT_8, &["8"]),
    (LocalActions::QUICK_SELECT_9, &["9"]),
    (LocalActions::QUICK_SELECT_10, &["0"]),
];

pub const LOCAL_SHORTCUT_PREF_KEYS: [(&str, &str); 23] = [
    (LocalActions::SEARCH, PrefsFields::LOCAL_SEARCH),
    (LocalActions::DELETE_ENTRY, PrefsFields::LOCAL_DELETE_ENTRY),
    (LocalActions::PRIVATE_MODE, PrefsFields::LOCAL_PRIVATE_MODE),
    (
        LocalActions::TOGGLE_FAVORITE,
        PrefsFields::LOCAL_TOGGLE_FAVORITE,
    ),
    (
        LocalActions::FAVORITES_VIEW,
        PrefsFields::LOCAL_FAVORITES_VIEW,
    ),
    (LocalActions::PAGE_NEXT, PrefsFields::LOCAL_PAGE_NEXT),
    (
        LocalActions::PAGE_PREVIOUS,
        PrefsFields::LOCAL_PAGE_PREVIOUS,
    ),
    (LocalActions::MOVE_UP, PrefsFields::LOCAL_MOVE_UP),
    (LocalActions::MOVE_DOWN, PrefsFields::LOCAL_MOVE_DOWN),
    (LocalActions::CONFIRM, PrefsFields::LOCAL_CONFIRM),
    (LocalActions::CLOSE, PrefsFields::LOCAL_CLOSE),
    (
        LocalActions::CASE_SENSITIVE,
        PrefsFields::LOCAL_CASE_SENSITIVE,
    ),
    (LocalActions::REGEX, PrefsFields::LOCAL_REGEX_SEARCH),
    (
        LocalActions::QUICK_SELECT_1,
        PrefsFields::LOCAL_QUICK_SELECT_1,
    ),
    (
        LocalActions::QUICK_SELECT_2,
        PrefsFields::LOCAL_QUICK_SELECT_2,
    ),
    (
        LocalActions::QUICK_SELECT_3,
        PrefsFields::LOCAL_QUICK_SELECT_3,
    ),
    (
        LocalActions::QUICK_SELECT_4,
        PrefsFields::LOCAL_QUICK_SELECT_4,
    ),
    (
        LocalActions::QUICK_SELECT_5,
        PrefsFields::LOCAL_QUICK_SELECT_5,
    ),
    (
        LocalActions::QUICK_SELECT_6,
        PrefsFields::LOCAL_QUICK_SELECT_6,
    ),
    (
        LocalActions::QUICK_SELECT_7,
        PrefsFields::LOCAL_QUICK_SELECT_7,
    ),
    (
        LocalActions::QUICK_SELECT_8,
        PrefsFields::LOCAL_QUICK_SELECT_8,
    ),
    (
        LocalActions::QUICK_SELECT_9,
        PrefsFields::LOCAL_QUICK_SELECT_9,
    ),
    (
        LocalActions::QUICK_SELECT_10,
        PrefsFields::LOCAL_QUICK_SELECT_10,
    ),
];

const F_KEY_DISPLAYS: [&str; 12] = [
    "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Accelerator {
    pub keyval: u32,
    pub mods: u32,
    pub display: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub keyval: u32,
    pub mods: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    pub keyval: u32,
    pub state: u32,
}

pub fn parse_accelerator(accelerator: &str) -> Option<Accelerator> {
    let mut mods = 0;
    let mut rest = accelerator;

    while rest.starts_with('<') {
        let tag_end = rest.find('>')?;
        let tag = &rest[1..tag_end];
        if tag.eq_ignore_ascii_case("control")
            || tag.eq_ignore_ascii_case("ctrl")
            || tag.eq_ignore_ascii_case("primary")
        {
            mods |= ModMask::CONTROL;
        } else if tag.eq_ignore_ascii_case("alt") {
            mods |= ModMask::MOD1;
        } else if tag.eq_ignore_ascii_case("shift") {
            mods |= ModMask::SHIFT;
        } else if tag.eq_ignore_ascii_case("super")
            || tag.eq_ignore_ascii_case("meta")
            || tag.eq_ignore_ascii_case("mod4")
        {
            mods |= ModMask::MOD4;
        } else {
            return None;
        }
        rest = &rest[tag_end + 1..];
    }

    if rest.is_empty() {
        return None;
    }

    if rest.encode_utf16().count() == 1 {
        let keyval = rest.chars().next()? as u32;
        return Some(Accelerator {
            keyval,
            mods,
            display: None,
        });
    }

    let (keyval, display) = named_key(rest)?;
    Some(Accelerator {
        keyval,
        mods,
        display: Some(display),
    })
}

pub fn parse_accelerator_list(accelerators: &[&str]) -> Vec<Binding> {
    accelerators
        .iter()
        .filter_map(|accelerator| parse_accelerator(accelerator))
        .map(|parsed| Binding {
            keyval: parsed.keyval,
            mods: parsed.mods,
        })
        .collect()
}

pub fn matches_binding(event: KeyEvent, binding: &Binding) -> bool {
    let keyval = event.keyval;
    let folded = keyval != binding.keyval
        && is_ascii_letter(keyval)
        && is_ascii_letter(binding.keyval)
        && (keyval | 0x20) == (binding.keyval | 0x20);
    if keyval != binding.keyval && !folded {
        return false;
    }
    if folded && binding.mods == 0 {
        return false;
    }
    let state = event.state & SIGNIFICANT_MODS;
    if state == binding.mods {
        return true;
    }
    folded && (binding.mods & ModMask::SHIFT) == 0 && state == (binding.mods | ModMask::SHIFT)
}

pub fn matches_shortcut(event: KeyEvent, bindings: &[Binding]) -> bool {
    bindings
        .iter()
        .any(|binding| matches_binding(event, binding))
}

pub fn format_accelerator(accelerator: &str) -> String {
    let Some(parsed) = parse_accelerator(accelerator) else {
        return accelerator.to_string();
    };

    let mut parts = Vec::new();
    if parsed.mods & ModMask::CONTROL != 0 {
        parts.push("Ctrl".to_string());
    }
    if parsed.mods & ModMask::MOD1 != 0 {
        parts.push("Alt".to_string());
    }
    if parsed.mods & ModMask::SHIFT != 0 {
        parts.push("Shift".to_string());
    }
    if parsed.mods & ModMask::MOD4 != 0 {
        parts.push("Super".to_string());
    }
    if parsed.mods & ModMask::MOD2 != 0 {
        parts.push("Mod2".to_string());
    }
    if parsed.mods & ModMask::MOD3 != 0 {
        parts.push("Mod3".to_string());
    }
    if parsed.mods & ModMask::MOD5 != 0 {
        parts.push("Mod5".to_string());
    }

    let key = if let Some(display) = parsed.display {
        display.to_string()
    } else {
        let key = char::from_u32(parsed.keyval)
            .unwrap_or('\u{fffd}')
            .to_string();
        if parsed.mods != 0 {
            key.to_uppercase()
        } else {
            key
        }
    };
    parts.push(key);
    parts.join("+")
}

pub fn default_local_shortcuts(action: &str) -> Option<&'static [&'static str]> {
    DEFAULT_LOCAL_SHORTCUTS
        .iter()
        .find(|(name, _)| *name == action)
        .map(|(_, shortcuts)| *shortcuts)
}

pub fn local_shortcut_pref_key(action: &str) -> Option<&'static str> {
    LOCAL_SHORTCUT_PREF_KEYS
        .iter()
        .find(|(name, _)| *name == action)
        .map(|(_, key)| *key)
}

fn is_ascii_letter(keyval: u32) -> bool {
    (0x41..=0x5a).contains(&keyval) || (0x61..=0x7a).contains(&keyval)
}

fn named_key(name: &str) -> Option<(u32, &'static str)> {
    let name = name.to_lowercase();
    let key = match name.as_str() {
        "tab" => (0xff09, "Tab"),
        "iso_left_tab" => (0xfe20, "Tab"),
        "return" => (0xff0d, "Return"),
        "kp_enter" => (0xff8d, "KP_Enter"),
        "escape" => (0xff1b, "Escape"),
        "backspace" => (0xff08, "BackSpace"),
        "delete" => (0xffff, "Delete"),
        "space" => (0x20, "Space"),
        "left" => (0xff51, "Left"),
        "up" => (0xff52, "Up"),
        "right" => (0xff53, "Right"),
        "down" => (0xff54, "Down"),
        "page_up" => (0xff55, "Page_Up"),
        "page_down" => (0xff56, "Page_Down"),
        "home" => (0xff50, "Home"),
        "end" => (0xff57, "End"),
        _ => {
            let number = name.strip_prefix('f')?.parse::<usize>().ok()?;
            if !(1..=12).contains(&number) {
                return None;
            }
            (0xffbd + number as u32, F_KEY_DISPLAYS[number - 1])
        }
    };
    Some(key)
}
