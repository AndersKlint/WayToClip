use gtk::prelude::*;

use crate::common::i18n::translate_native;

use crate::clipboard::clipboard_entry::ClipboardEntry;

const PREVIEW_CHARS_PER_LINE: usize = 60;
const PREVIEW_ELLIPSIS: &str = " ...";

pub struct PopupUi {
    pub root: gtk::Box,
    pub list: gtk::Box,
    pub list_scroll: gtk::ScrolledWindow,
    pub search_bar: gtk::Box,
    pub search_entry: gtk::Entry,
    pub case_button: gtk::ToggleButton,
    pub regex_button: gtk::ToggleButton,
    pub search_hint: gtk::Box,
    pub search_hint_label: gtk::Label,
    pub private_hint: gtk::Box,
    pub private_hint_label: gtk::Label,
    pub favorite_hint: gtk::Box,
    pub favorite_hint_label: gtk::Label,
    pub delete_hint: gtk::Box,
    pub delete_hint_label: gtk::Label,
    pub page_indicator: gtk::Label,
    pub status: gtk::Label,
}

impl PopupUi {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("waytoclip-popup");
        root.set_width_request(400);
        root.set_focusable(true);

        let search_bar = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        search_bar.add_css_class("waytoclip-search-bar");
        search_bar.set_visible(false);

        let search_entry = gtk::Entry::builder()
            .placeholder_text(translate_native("Search..."))
            .hexpand(true)
            .build();
        search_entry.add_css_class("waytoclip-search-entry");

        let case_button = gtk::ToggleButton::with_label("Aa");
        case_button.add_css_class("waytoclip-search-toggle");
        case_button.set_focus_on_click(false);
        case_button.set_tooltip_text(Some(&translate_native("Match Case (Alt+C)")));

        let regex_button = gtk::ToggleButton::with_label(".*");
        regex_button.add_css_class("waytoclip-search-toggle");
        regex_button.set_focus_on_click(false);
        regex_button.set_tooltip_text(Some(&translate_native("Use Regular Expression (Alt+R)")));

        search_bar.append(&search_entry);
        search_bar.append(&case_button);
        search_bar.append(&regex_button);

        let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
        list.add_css_class("waytoclip-popup-list");

        let list_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .child(&list)
            .build();
        list_scroll.add_css_class("waytoclip-popup-list-scroll");

        let (search_hint, search_hint_label) = create_hint("system-search-symbolic", " = s");
        let (private_hint, private_hint_label) = create_hint("security-medium-symbolic", " = p");
        let (favorite_hint, favorite_hint_label) = create_hint("starred-symbolic", " = f");
        let (delete_hint, delete_hint_label) = create_hint("user-trash-symbolic", " = d");

        let left_hints = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        left_hints.append(&search_hint);
        left_hints.append(&private_hint);

        let page_indicator = gtk::Label::new(None);
        page_indicator.add_css_class("waytoclip-page-indicator");
        page_indicator.set_hexpand(true);
        page_indicator.set_xalign(0.5);
        page_indicator.set_visible(false);

        let right_hints = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        right_hints.append(&favorite_hint);
        right_hints.append(&delete_hint);

        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        footer.append(&left_hints);
        footer.append(&page_indicator);
        footer.append(&right_hints);
        let settings_button = gtk::Button::from_icon_name("emblem-system-symbolic");
        settings_button.add_css_class("flat");
        settings_button.set_focus_on_click(false);
        settings_button.set_tooltip_text(Some(&translate_native("Settings")));
        settings_button.set_action_name(Some("app.settings"));
        footer.append(&settings_button);

        let status = gtk::Label::new(None);
        status.add_css_class("waytoclip-status");
        status.set_wrap(true);
        status.set_xalign(0.0);
        status.set_visible(false);

        root.append(&search_bar);
        root.append(&list_scroll);
        root.append(&footer);
        root.append(&status);

        Self {
            root,
            list,
            list_scroll,
            search_bar,
            search_entry,
            case_button,
            regex_button,
            search_hint,
            search_hint_label,
            private_hint,
            private_hint_label,
            favorite_hint,
            favorite_hint_label,
            delete_hint,
            delete_hint_label,
            page_indicator,
            status,
        }
    }

    pub fn create_item_row(
        &self,
        entry: &ClipboardEntry,
        image_preview: Option<gtk::Widget>,
        slot: usize,
        shortcut_label: Option<&str>,
        max_lines: i32,
        on_activate: impl Fn() + 'static,
    ) -> gtk::Box {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.add_css_class("waytoclip-popup-item");

        let number_text = match shortcut_label {
            Some(label) if label.is_empty() => String::new(),
            Some(label) => format!("{label}. "),
            None => format!("{}. ", (slot + 1) % 10),
        };
        let number = gtk::Label::new(Some(&number_text));
        number.add_css_class("waytoclip-item-number");
        number.set_valign(gtk::Align::Start);

        let text_container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        text_container.add_css_class("waytoclip-item-text-container");
        text_container.set_hexpand(true);
        text_container.set_valign(gtk::Align::Center);

        if let Some(preview) = image_preview {
            text_container.append(&preview);
        } else {
            let label = gtk::Label::new(Some(&truncate_preview_text(
                &entry.get_string_value(),
                max_lines,
            )));
            label.add_css_class("waytoclip-item-text");
            label.set_wrap(true);
            label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_lines(max_lines);
            label.set_max_width_chars(PREVIEW_CHARS_PER_LINE as i32);
            label.set_xalign(0.0);
            label.set_hexpand(true);
            text_container.append(&label);
        }

        row.append(&number);
        row.append(&text_container);

        let gesture = gtk::GestureClick::new();
        gesture.connect_released(move |_, _, _, _| on_activate());
        row.add_controller(gesture);
        row
    }

    pub fn create_favorites_row(
        &self,
        is_back: bool,
        on_activate: impl Fn() + 'static,
    ) -> gtk::Box {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.add_css_class("waytoclip-popup-item");
        row.add_css_class("waytoclip-favorites-row");

        let icon = gtk::Image::from_icon_name(if is_back {
            "go-previous-symbolic"
        } else {
            "folder-symbolic"
        });
        icon.add_css_class("waytoclip-favorites-icon");

        let label = gtk::Label::new(Some(&if is_back {
            translate_native("Back")
        } else {
            translate_native("Favorites")
        }));
        label.add_css_class("waytoclip-item-text");
        label.set_xalign(0.0);
        label.set_hexpand(true);

        row.append(&icon);
        row.append(&label);

        let gesture = gtk::GestureClick::new();
        gesture.connect_released(move |_, _, _, _| on_activate());
        row.add_controller(gesture);
        row
    }

    pub fn create_empty_label(&self, text: &str) -> gtk::Label {
        let label = gtk::Label::new(Some(text));
        label.add_css_class("waytoclip-empty-label");
        label.set_halign(gtk::Align::Center);
        label.set_hexpand(true);
        label
    }

    pub fn set_search_visible(&self, is_visible: bool) {
        self.search_bar.set_visible(is_visible);
    }

    pub fn set_search_toggle_state(&self, button: &gtk::ToggleButton, is_active: bool) {
        button.set_active(is_active);
        if is_active {
            button.add_css_class("active");
        } else {
            button.remove_css_class("active");
        }
    }

    pub fn set_page_indicator(&self, current_page: usize, total_pages: usize, is_visible: bool) {
        self.page_indicator
            .set_text(&format!("{} / {}", current_page + 1, total_pages));
        self.page_indicator.set_visible(is_visible);
    }

    pub fn set_status(&self, message: &str) {
        self.status.set_text(message);
        self.status.set_visible(!message.is_empty());
    }

    pub fn set_private_hint_active(&self, is_active: bool) {
        set_active_class(&self.private_hint, is_active);
    }

    pub fn set_favorite_hint_active(&self, is_active: bool) {
        set_active_class(&self.favorite_hint, is_active);
    }
}

fn create_hint(icon_name: &str, text: &str) -> (gtk::Box, gtk::Label) {
    let hint = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    hint.add_css_class("waytoclip-hint");
    hint.set_valign(gtk::Align::Center);

    let icon = gtk::Image::from_icon_name(icon_name);
    let label = gtk::Label::new(Some(text));
    hint.append(&icon);
    hint.append(&label);
    (hint, label)
}

fn set_active_class(widget: &impl IsA<gtk::Widget>, is_active: bool) {
    if is_active {
        widget.add_css_class("active");
    } else {
        widget.remove_css_class("active");
    }
}

pub fn truncate_preview_text(text: &str, max_lines: i32) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    if normalized.is_empty() {
        return String::new();
    }
    let lines = max_lines.clamp(1, 3) as usize;

    let mut is_truncated = false;
    let mut preview = if lines <= 1 {
        normalized.replace('\n', " ")
    } else {
        let mut parts: Vec<&str> = normalized.split('\n').collect();
        if parts.len() > lines {
            parts.truncate(lines);
            is_truncated = true;
        }
        parts.join("\n")
    };

    let max_chars = lines * PREVIEW_CHARS_PER_LINE;
    if preview.chars().count() > max_chars {
        preview = preview.chars().take(max_chars).collect();
        preview = preview.trim_end().to_string();
        is_truncated = true;
    }

    if is_truncated {
        preview = preview.trim_end().to_string();
        if !preview.ends_with("...") {
            preview.push_str(PREVIEW_ELLIPSIS);
        }
    }
    preview
}
