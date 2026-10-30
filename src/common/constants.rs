pub struct PrefsFields;

impl PrefsFields {
    pub const HISTORY_SIZE: &'static str = "history-size";
    pub const CACHE_FILE_SIZE: &'static str = "cache-size";
    pub const CACHE_ONLY_FAVORITE: &'static str = "cache-only-favorites";
    pub const CONFIRM_ON_CLEAR: &'static str = "confirm-clear";
    pub const MOVE_ITEM_FIRST: &'static str = "move-item-first";
    pub const ENABLE_KEYBINDING: &'static str = "enable-keybindings";
    pub const KEEP_SELECTED_ON_CLEAR: &'static str = "keep-selected-on-clear";
    pub const BINDING_TOGGLE_POPUP: &'static str = "toggle-popup";
    pub const BINDING_CLEAR_HISTORY: &'static str = "clear-history";
    pub const BINDING_PRIVATE_MODE: &'static str = "private-mode-binding";
    pub const CLEAR_ON_BOOT: &'static str = "clear-on-boot";
    pub const AUTO_PASTE: &'static str = "auto-paste";
    pub const POPUP_POSITION_MODE: &'static str = "popup-position-mode";
    pub const MAX_POPUP_PAGES: &'static str = "popup-pages";
    pub const LIMIT_POPUP_PAGES: &'static str = "limit-popup-pages";
    pub const SHOULD_CACHE_IMAGES: &'static str = "should-cache-images";
    pub const EXCLUDED_APPS: &'static str = "excluded-apps";
    pub const TERMINAL_APPS: &'static str = "terminal-apps";
    pub const CLEAR_HISTORY_ON_INTERVAL: &'static str = "clear-history-on-interval";
    pub const CLEAR_HISTORY_INTERVAL: &'static str = "clear-history-interval";
    pub const NEXT_HISTORY_CLEAR: &'static str = "next-history-clear";
    pub const CASE_SENSITIVE_SEARCH: &'static str = "case-sensitive-search";
    pub const REGEX_SEARCH: &'static str = "regex-search";
    pub const IMAGE_PREVIEW_SIZE: &'static str = "image-preview-size";
    pub const SHOW_SHORTCUT_HINTS: &'static str = "show-shortcut-hints";
    pub const IGNORE_SECRET_MIMETYPES: &'static str = "ignore-secret-mimetypes";
    pub const LOCAL_SEARCH: &'static str = "local-search";
    pub const LOCAL_DELETE_ENTRY: &'static str = "local-delete-entry";
    pub const LOCAL_PRIVATE_MODE: &'static str = "local-private-mode";
    pub const LOCAL_PAGE_NEXT: &'static str = "local-page-next";
    pub const LOCAL_PAGE_PREVIOUS: &'static str = "local-page-previous";
    pub const LOCAL_MOVE_UP: &'static str = "local-move-up";
    pub const LOCAL_MOVE_DOWN: &'static str = "local-move-down";
    pub const LOCAL_CONFIRM: &'static str = "local-confirm";
    pub const LOCAL_CLOSE: &'static str = "local-close";
    pub const LOCAL_CASE_SENSITIVE: &'static str = "local-case-sensitive";
    pub const LOCAL_REGEX_SEARCH: &'static str = "local-regex-search";
    pub const LOCAL_TOGGLE_FAVORITE: &'static str = "local-toggle-favorite";
    pub const LOCAL_FAVORITES_VIEW: &'static str = "local-favorites-view";
    pub const LOCAL_QUICK_SELECT_1: &'static str = "local-quick-select-1";
    pub const LOCAL_QUICK_SELECT_2: &'static str = "local-quick-select-2";
    pub const LOCAL_QUICK_SELECT_3: &'static str = "local-quick-select-3";
    pub const LOCAL_QUICK_SELECT_4: &'static str = "local-quick-select-4";
    pub const LOCAL_QUICK_SELECT_5: &'static str = "local-quick-select-5";
    pub const LOCAL_QUICK_SELECT_6: &'static str = "local-quick-select-6";
    pub const LOCAL_QUICK_SELECT_7: &'static str = "local-quick-select-7";
    pub const LOCAL_QUICK_SELECT_8: &'static str = "local-quick-select-8";
    pub const LOCAL_QUICK_SELECT_9: &'static str = "local-quick-select-9";
    pub const LOCAL_QUICK_SELECT_10: &'static str = "local-quick-select-10";
    pub const ENABLE_FAVORITES: &'static str = "enable-favorites";
    pub const LANGUAGE: &'static str = "language";
}

pub const ITEMS_PER_PAGE: usize = 10;

pub const CLIPBOARD_MIMETYPES: [&str; 11] = [
    "text/plain;charset=utf-8",
    "UTF8_STRING",
    "text/plain",
    "STRING",
    "image/gif",
    "image/png",
    "image/jpg",
    "image/jpeg",
    "image/webp",
    "image/svg+xml",
    "text/html",
];

pub struct PopupPositionMode;

impl PopupPositionMode {
    pub const CURSOR: i32 = 0;
    pub const WINDOW_CENTER: i32 = 1;
}
