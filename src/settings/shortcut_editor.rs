use std::{cell::RefCell, rc::Rc};

use gtk::{gdk, gio, glib, prelude::*};

use super::shortcut_highlighter::ShortcutHighlighter;
use crate::common::i18n::translate_native as tr;

pub const GLOBAL_SHORTCUTS: &[(&str, &str)] = &[
    ("toggle-popup", "Toggle the clipboard popup"),
    ("private-mode-binding", "Private mode"),
    ("clear-history", "Clear history"),
];

pub const LOCAL_SHORTCUTS: &[(&str, &str)] = &[
    ("local-search", "Search in popup"),
    ("local-delete-entry", "Delete selected entry"),
    ("local-private-mode", "Toggle private mode"),
    ("local-page-next", "Next page"),
    ("local-page-previous", "Previous page"),
    ("local-move-up", "Move selection up"),
    ("local-move-down", "Move selection down"),
    ("local-confirm", "Confirm selection"),
    ("local-close", "Close popup"),
    ("local-case-sensitive", "Match case (while searching)"),
    (
        "local-regex-search",
        "Use regular expression (while searching)",
    ),
    ("local-toggle-favorite", "Toggle favorite on selected entry"),
    ("local-favorites-view", "Open favorites or go back"),
    ("local-quick-select-1", "Quick select item 1"),
    ("local-quick-select-2", "Quick select item 2"),
    ("local-quick-select-3", "Quick select item 3"),
    ("local-quick-select-4", "Quick select item 4"),
    ("local-quick-select-5", "Quick select item 5"),
    ("local-quick-select-6", "Quick select item 6"),
    ("local-quick-select-7", "Quick select item 7"),
    ("local-quick-select-8", "Quick select item 8"),
    ("local-quick-select-9", "Quick select item 9"),
    ("local-quick-select-10", "Quick select item 10"),
];

pub struct ShortcutEditor {
    pub widget: gtk::Box,
    pub add_button: gtk::Button,
    settings: gio::Settings,
    key: &'static str,
    parent: gtk::ApplicationWindow,
    application: gtk::Application,
    chips: gtk::Box,
    dialog: RefCell<Option<gtk::Window>>,
    changed: RefCell<Option<glib::SignalHandlerId>>,
    capture_ready_changed: RefCell<Option<glib::SignalHandlerId>>,
    capture_status_binding: RefCell<Option<glib::Binding>>,
    error: gtk::Label,
    shortcut_status: gtk::Label,
    highlighter: ShortcutHighlighter,
}

impl ShortcutEditor {
    pub fn new(
        settings: &gio::Settings,
        key: &'static str,
        parent: &gtk::ApplicationWindow,
        application: &gtk::Application,
        error: &gtk::Label,
        shortcut_status: &gtk::Label,
    ) -> Rc<Self> {
        let widget = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let chips = gtk::Box::new(gtk::Orientation::Vertical, 4);
        widget.append(&chips);
        let add_button = gtk::Button::from_icon_name("list-add-symbolic");
        add_button.set_tooltip_text(Some(&tr("Add shortcut")));
        widget.append(&add_button);
        let editor = Rc::new(Self {
            widget,
            chips,
            add_button,
            settings: settings.clone(),
            key,
            parent: parent.clone(),
            application: application.clone(),
            dialog: RefCell::new(None),
            changed: RefCell::new(None),
            capture_ready_changed: RefCell::new(None),
            capture_status_binding: RefCell::new(None),
            error: error.clone(),
            shortcut_status: shortcut_status.clone(),
            highlighter: ShortcutHighlighter::default(),
        });
        let weak = Rc::downgrade(&editor);
        editor.add_button.connect_clicked(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.capture();
            }
        });
        let weak = Rc::downgrade(&editor);
        let id = settings.connect_changed(Some(key), move |_, _| {
            if let Some(editor) = weak.upgrade() {
                editor.render();
            }
        });
        editor.changed.replace(Some(id));
        editor.render();
        editor
    }

    fn render(self: &Rc<Self>) {
        while let Some(child) = self.chips.first_child() {
            self.chips.remove(&child);
        }
        let shortcuts = self.settings.strv(self.key);
        if shortcuts.is_empty() {
            let label = gtk::Label::new(Some(&tr("Disabled")));
            label.add_css_class("dim-label");
            self.chips.append(&label);
        }
        for shortcut in shortcuts.iter() {
            let chip = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            let label = gtk::accelerator_parse(shortcut.as_str())
                .map(|(key, modifiers)| gtk::accelerator_get_label(key, modifiers).to_string())
                .unwrap_or_else(|| shortcut.to_string());
            chip.append(&gtk::Label::new(Some(&label)));
            let remove = gtk::Button::from_icon_name("window-close-symbolic");
            remove.set_tooltip_text(Some(&tr("Remove shortcut")));
            chip.append(&remove);
            let weak = Rc::downgrade(self);
            let shortcut = shortcut.to_string();
            remove.connect_clicked(move |_| {
                if let Some(editor) = weak.upgrade() {
                    let values: Vec<String> = editor
                        .settings
                        .strv(editor.key)
                        .iter()
                        .filter(|value| value.as_str() != shortcut)
                        .map(ToString::to_string)
                        .collect();
                    editor.write(&values);
                }
            });
            self.chips.append(&chip);
        }
        if !shortcuts.is_empty() {
            self.highlighter.clear();
        }
    }

    pub fn highlight_if_unset(&self) {
        if self.key == "toggle-popup" && self.settings.strv(self.key).is_empty() {
            self.highlighter.highlight(&self.add_button);
        }
    }

    fn write(&self, values: &[String]) -> bool {
        match self.settings.set_strv(self.key, values) {
            Ok(()) => {
                self.error.set_text("");
                true
            }
            Err(error) => {
                self.error.set_text(&error.to_string());
                false
            }
        }
    }

    fn capture(self: &Rc<Self>) {
        if let Some(dialog) = self.dialog.borrow().as_ref() {
            dialog.present();
            return;
        }
        if !self.application.has_action("set-shortcut-capture")
            || !self.application.has_action("shortcut-capture-ready")
        {
            self.error
                .set_text("Shortcut capture is unavailable: the capture actions are missing.");
            return;
        }
        let dialog = gtk::Window::builder()
            .title(tr("Add shortcut"))
            .transient_for(&self.parent)
            .modal(true)
            .default_width(420)
            .build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.set_margin_top(18);
        content.set_margin_bottom(18);
        content.set_margin_start(18);
        content.set_margin_end(18);
        let prompt = gtk::Label::new(Some(&tr("Preparing shortcut capture…")));
        let explanation = gtk::Label::new(Some(&tr(
            "Waiting for the desktop to suspend global shortcuts. If the backend is unavailable or suspension fails, cancel and try again when it is available.",
        )));
        explanation.set_wrap(true);
        let status = gtk::Label::new(None);
        status.set_wrap(true);
        let binding = self
            .shortcut_status
            .bind_property("label", &status, "label")
            .sync_create()
            .build();
        self.capture_status_binding.replace(Some(binding));
        let message = gtk::Label::new(None);
        message.set_wrap(true);
        let cancel = gtk::Button::with_label(&tr("Cancel"));
        content.append(&prompt);
        content.append(&explanation);
        content.append(&status);
        content.append(&message);
        content.append(&cancel);
        dialog.set_child(Some(&content));
        let pending = Rc::new(RefCell::new(None::<String>));
        let controller = gtk::EventControllerKey::new();
        controller.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        let pending_press = pending.clone();
        let prompt_press = prompt.clone();
        controller.connect_key_pressed(move |_, key, _, state| {
            let state = state & gtk::accelerator_get_default_mod_mask();
            if let Some(editor) = weak.upgrade() {
                if state.is_empty() && (key == gdk::Key::Escape || key == gdk::Key::BackSpace) {
                    editor.finish_capture();
                    return glib::Propagation::Stop;
                }
                if !editor.is_capture_ready() {
                    pending_press.borrow_mut().take();
                    return glib::Propagation::Stop;
                }
                if let Some(mut shortcut) = normalized(key, state) {
                    if !editor.is_global() {
                        if let Some(character) = key.to_lower().to_unicode().filter(|character| {
                            !character.is_control() && !character.is_whitespace()
                        }) {
                            if let Some(name) = key.to_lower().name() {
                                if let Some(prefix) = shortcut.strip_suffix(name.as_str()) {
                                    shortcut = format!("{prefix}{character}");
                                }
                            }
                        }
                    }
                    prompt_press.set_text(&gtk::accelerator_get_label(key, state));
                    pending_press.replace(Some(shortcut));
                }
            }
            glib::Propagation::Stop
        });
        let weak = Rc::downgrade(self);
        let pending_state = pending.clone();
        controller.connect_key_released(move |_, _, _, _| {
            if let Some(editor) = weak.upgrade() {
                if !editor.is_capture_ready() {
                    pending.borrow_mut().take();
                    return;
                }
                let shortcut = pending.borrow_mut().take();
                if let Some(shortcut) = shortcut {
                    match editor.validate(&shortcut) {
                        Ok(()) => {
                            let mut values: Vec<String> = editor
                                .settings
                                .strv(editor.key)
                                .iter()
                                .map(ToString::to_string)
                                .collect();
                            values.push(shortcut);
                            if editor.write(&values) {
                                editor.finish_capture();
                            } else {
                                message.set_text(&editor.error.text());
                            }
                        }
                        Err(error) => message.set_text(&error),
                    }
                } else {
                    message.set_text("Press a key other than a modifier to record a shortcut.");
                }
            }
        });
        dialog.add_controller(controller);
        let weak = Rc::downgrade(self);
        cancel.connect_clicked(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.finish_capture();
            }
        });
        let weak = Rc::downgrade(self);
        dialog.connect_close_request(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.finish_capture();
            }
            glib::Propagation::Stop
        });
        let weak = Rc::downgrade(self);
        dialog.connect_destroy(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.finish_capture();
            }
        });
        self.dialog.replace(Some(dialog.clone()));
        let prompt_state = prompt.clone();
        let explanation_state = explanation.clone();
        let id = self.application.connect_action_state_changed(
            Some("shortcut-capture-ready"),
            move |_, _, state| {
                pending_state.borrow_mut().take();
                let is_ready = state.get::<bool>() == Some(true);
                prompt_state.set_text(&tr(if is_ready {
                    "Enter shortcut…"
                } else {
                    "Preparing shortcut capture…"
                }));
                explanation_state.set_visible(!is_ready);
            },
        );
        self.capture_ready_changed.replace(Some(id));
        self.application
            .activate_action("set-shortcut-capture", Some(&true.to_variant()));
        let is_ready = self.is_capture_ready();
        prompt.set_text(&tr(if is_ready {
            "Enter shortcut…"
        } else {
            "Preparing shortcut capture…"
        }));
        explanation.set_visible(!is_ready);
        dialog.present();
    }

    fn validate(&self, shortcut: &str) -> Result<(), String> {
        if !self.is_global()
            && crate::cursor_popup::local_shortcut_utils::parse_accelerator(shortcut).is_none()
        {
            return Err("This key is not supported by the popup shortcut handler.".into());
        }
        if self
            .settings
            .strv(self.key)
            .iter()
            .any(|value| canonical(value) == canonical(shortcut))
        {
            return Err("This shortcut is already assigned to this action.".into());
        }
        if self.is_global() {
            for (key, title) in GLOBAL_SHORTCUTS {
                if *key != self.key
                    && self
                        .settings
                        .strv(key)
                        .iter()
                        .any(|value| canonical(value) == canonical(shortcut))
                {
                    return Err(format!(
                        "This shortcut is already assigned to {}.",
                        tr(title)
                    ));
                }
            }
        }
        Ok(())
    }

    fn is_global(&self) -> bool {
        GLOBAL_SHORTCUTS.iter().any(|(key, _)| *key == self.key)
    }

    fn is_capture_ready(&self) -> bool {
        self.application
            .action_state("shortcut-capture-ready")
            .and_then(|state| state.get::<bool>())
            == Some(true)
    }

    pub fn finish_capture(&self) {
        self.highlighter.clear();
        let dialog = self.dialog.borrow_mut().take();
        if let Some(dialog) = dialog {
            if let Some(id) = self.capture_ready_changed.borrow_mut().take() {
                self.application.disconnect(id);
            }
            if let Some(binding) = self.capture_status_binding.borrow_mut().take() {
                binding.unbind();
            }
            dialog.destroy();
            self.application
                .activate_action("set-shortcut-capture", Some(&false.to_variant()));
        }
    }

    pub fn disconnect(&self) {
        self.highlighter.clear();
        self.finish_capture();
        if let Some(id) = self.changed.borrow_mut().take() {
            self.settings.disconnect(id);
        }
    }
}

impl Drop for ShortcutEditor {
    fn drop(&mut self) {
        self.disconnect();
    }
}

fn normalized(key: gdk::Key, modifiers: gdk::ModifierType) -> Option<String> {
    let name = key.name()?;
    if matches!(
        name.as_str(),
        "Shift_L"
            | "Shift_R"
            | "Control_L"
            | "Control_R"
            | "Alt_L"
            | "Alt_R"
            | "Meta_L"
            | "Meta_R"
            | "Super_L"
            | "Super_R"
            | "Hyper_L"
            | "Hyper_R"
            | "Caps_Lock"
            | "Num_Lock"
            | "ISO_Level3_Shift"
            | "ISO_Level5_Shift"
    ) {
        return None;
    }
    Some(gtk::accelerator_name(key.to_lower(), modifiers).to_string())
}

fn canonical(value: &str) -> String {
    gtk::accelerator_parse(value)
        .and_then(|(key, modifiers)| normalized(key, modifiers))
        .unwrap_or_else(|| value.to_string())
}
