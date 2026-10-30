use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use gtk::{glib, prelude::*};

use super::{
    SettingsManager,
    controls::{self, Control},
    shortcut_editor::{GLOBAL_SHORTCUTS, LOCAL_SHORTCUTS, ShortcutEditor},
    string_list_manager::StringListManager,
};
use crate::common::i18n::{self, translate_native as tr};

pub struct SettingsWindow {
    window: gtk::ApplicationWindow,
    settings: Rc<SettingsManager>,
    controls: RefCell<Vec<Control>>,
    shortcuts: RefCell<Vec<Rc<ShortcutEditor>>>,
    lists: RefCell<Vec<Rc<StringListManager>>>,
    changed: RefCell<Option<glib::SignalHandlerId>>,
    is_syncing: Cell<bool>,
    error: gtk::Label,
    shortcut_status: gtk::Label,
    self_weak: Weak<Self>,
    labels_language: RefCell<String>,
}

impl SettingsWindow {
    pub fn new(application: &gtk::Application, settings: Rc<SettingsManager>) -> Rc<Self> {
        let window = gtk::ApplicationWindow::builder()
            .application(application)
            .title("WayToClip Settings")
            .default_width(780)
            .default_height(760)
            .hide_on_close(true)
            .build();
        window.set_widget_name("waytoclip-settings");
        let error = gtk::Label::new(None);
        error.set_wrap(true);
        error.add_css_class("error");
        let shortcut_status = gtk::Label::new(None);
        shortcut_status.set_wrap(true);
        shortcut_status.set_xalign(0.0);
        let labels_language = i18n::get_stored_language(settings.gio());
        let owner = Rc::new_cyclic(|weak| Self {
            window,
            settings,
            controls: RefCell::new(Vec::new()),
            shortcuts: RefCell::new(Vec::new()),
            lists: RefCell::new(Vec::new()),
            changed: RefCell::new(None),
            is_syncing: Cell::new(false),
            error,
            shortcut_status,
            self_weak: weak.clone(),
            labels_language: RefCell::new(labels_language),
        });
        owner.build(application);
        let weak = Rc::downgrade(&owner);
        let id = owner.settings.gio().connect_changed(None, move |_, _| {
            if let Some(owner) = weak.upgrade() {
                owner.sync();
            }
        });
        owner.changed.replace(Some(id));
        let weak = Rc::downgrade(&owner);
        owner.window.connect_close_request(move |_| {
            if let Some(owner) = weak.upgrade() {
                owner.close();
            }
            glib::Propagation::Stop
        });
        let weak = Rc::downgrade(&owner);
        owner.window.connect_destroy(move |_| {
            if let Some(owner) = weak.upgrade() {
                owner.disconnect();
            }
        });
        let weak = Rc::downgrade(&owner);
        owner.window.connect_map(move |_| {
            if let Some(owner) = weak.upgrade() {
                for editor in owner.shortcuts.borrow().iter() {
                    editor.highlight_if_unset();
                }
            }
        });
        owner.sync();
        owner
    }

    fn build(self: &Rc<Self>, application: &gtk::Application) {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
        content.set_margin_start(24);
        content.set_margin_end(24);
        content.set_margin_bottom(24);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&content)
            .build();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.append(&self.error);
        scroll.set_vexpand(true);
        root.append(&scroll);
        self.window.set_child(Some(&root));
        let general = controls::group(&content, "General", "");
        let codes: Vec<String> = i18n::available_languages()
            .into_iter()
            .map(str::to_string)
            .collect();
        let labels: Vec<String> = codes
            .iter()
            .map(|code| {
                i18n::language_label(code)
                    .map(str::to_string)
                    .unwrap_or_else(|| tr("System"))
            })
            .collect();
        self.choice(
            &general,
            "language",
            "Language",
            "Reopen Settings to update its labels",
            &labels,
            codes,
        );
        let popup = controls::group(&content, "Popup", "");
        self.choice(
            &popup,
            "popup-position-mode",
            "Popup position",
            "Where to show the clipboard popup",
            &[tr("At mouse cursor"), tr("Center of focused window")],
            vec![],
        );
        self.boolean(
            &popup,
            "limit-popup-pages",
            "Limit number of pages",
            "When off, the popup shows the full clipboard history",
        );
        self.integer(
            &popup,
            "popup-pages",
            "Number of pages",
            "Each page shows 10 items (Tab to navigate pages)",
            1.0,
        );
        self.integer(
            &popup,
            "image-preview-size",
            "Image thumbnail size (px)",
            "Preview size for images in the popup",
            8.0,
        );
        self.boolean(
            &popup,
            "enable-favorites",
            "Enable favorites",
            "Mark entries as favorites and browse them in the popup",
        );
        let behavior = controls::group(&content, "Behavior", "");
        self.boolean(
            &behavior,
            "auto-paste",
            "Auto-paste on selection",
            "Automatically paste after selecting an item from the popup",
        );
        self.string_list(&behavior, "terminal-apps", "Terminal Apps", "Auto-paste uses Ctrl+Shift+V in terminals. Add your terminal if pasting inserts the wrong text", "Window class or app id, e.g. \"org.gnome.Ptyxis\"", false);
        for (key, title, subtitle) in [
            (
                "move-item-first",
                "Move item to the top after selection",
                "",
            ),
            (
                "keep-selected-on-clear",
                "Keep selected entry after Clear History",
                "",
            ),
            (
                "should-cache-images",
                "Cache images",
                "Capture images and save them to disk so they survive a restart",
            ),
            ("cache-only-favorites", "Only save favorites to disk", ""),
            ("clear-on-boot", "Clear history when WayToClip starts", ""),
            (
                "clear-history-on-interval",
                "Clear clipboard history on interval",
                "",
            ),
        ] {
            self.boolean(&behavior, key, title, subtitle);
        }
        self.integer(
            &behavior,
            "clear-history-interval",
            "History clear interval (in minutes)",
            "",
            10.0,
        );
        self.boolean(
            &behavior,
            "confirm-clear",
            "Show confirmation on Clear History",
            "",
        );
        let limits = controls::group(&content, "Limits", "");
        self.integer(&limits, "history-size", "History Size", "", 1.0);
        self.integer(&limits, "cache-size", "Max cache file size (MB)", "", 1.0);
        let exclusion = controls::group(&content, "Exclusion", "");
        self.boolean(
            &exclusion,
            "ignore-secret-mimetypes",
            "Ignore secret clipboard types",
            "Skip content marked with a secret mimetype (password managers)",
        );
        self.string_list(
            &exclusion,
            "excluded-apps",
            "Excluded Apps",
            "Content copied will not be saved while these apps are in focus",
            "Window class name, e.g. \"KeePassXC\"",
            true,
        );
        let globals = controls::group(
            &content,
            "Global shortcuts",
            "Disabled by default; assign a shortcut below to enable each action",
        );
        self.boolean(&globals, "enable-keybindings", "Enable shortcuts", "");
        for (key, title) in GLOBAL_SHORTCUTS {
            self.shortcut(&globals, key, title, application);
        }
        globals.append(&self.shortcut_status);
        self.reset(
            &globals,
            GLOBAL_SHORTCUTS.iter().map(|(key, _)| *key).collect(),
        );
        let locals = controls::group(&content, "Popup shortcuts", "");
        self.boolean(
            &locals,
            "show-shortcut-hints",
            "Show shortcut reminder icons",
            "Show the shortcut reminder icons at the bottom of the popup",
        );
        for (key, title) in LOCAL_SHORTCUTS {
            self.shortcut(&locals, key, title, application);
        }
        self.reset(
            &locals,
            LOCAL_SHORTCUTS.iter().map(|(key, _)| *key).collect(),
        );
    }

    fn boolean(
        self: &Rc<Self>,
        group: &gtk::ListBox,
        key: &'static str,
        title: &str,
        subtitle: &str,
    ) {
        let widget = gtk::Switch::new();
        group.append(&controls::row(title, subtitle, &widget));
        let weak = Rc::downgrade(self);
        widget.connect_active_notify(move |widget| {
            if let Some(owner) = weak.upgrade() {
                if !owner.is_syncing.get()
                    && owner.settings.gio().boolean(key) != widget.is_active()
                {
                    owner.report(owner.settings.gio().set_boolean(key, widget.is_active()));
                }
            }
        });
        self.controls
            .borrow_mut()
            .push(Control::Boolean(key, widget));
    }

    fn integer(
        self: &Rc<Self>,
        group: &gtk::ListBox,
        key: &'static str,
        title: &str,
        subtitle: &str,
        step: f64,
    ) {
        let schema = self
            .settings
            .gio()
            .settings_schema()
            .expect("SettingsManager owns a schema");
        let range = schema
            .key(key)
            .range()
            .child_value(1)
            .get::<glib::Variant>()
            .expect("schema range contains a variant");
        let (lower, upper) = range
            .get::<(i32, i32)>()
            .expect("integer setting has a schema range");
        let widget = gtk::SpinButton::with_range(lower as f64, upper as f64, step);
        widget.set_numeric(true);
        widget.set_width_chars(9);
        group.append(&controls::row(title, subtitle, &widget));
        let weak = Rc::downgrade(self);
        widget.connect_value_changed(move |widget| {
            if let Some(owner) = weak.upgrade() {
                if !owner.is_syncing.get() && owner.settings.gio().int(key) != widget.value_as_int()
                {
                    owner.report(owner.settings.gio().set_int(key, widget.value_as_int()));
                }
            }
        });
        self.controls
            .borrow_mut()
            .push(Control::Integer(key, widget));
    }

    fn choice(
        self: &Rc<Self>,
        group: &gtk::ListBox,
        key: &'static str,
        title: &str,
        subtitle: &str,
        labels: &[String],
        values: Vec<String>,
    ) {
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        let widget = gtk::DropDown::from_strings(&labels);
        group.append(&controls::row(title, subtitle, &widget));
        let weak = Rc::downgrade(self);
        let codes = values.clone();
        widget.connect_selected_notify(move |widget| {
            if let Some(owner) = weak.upgrade() {
                if owner.is_syncing.get() {
                    return;
                }
                let settings = owner.settings.gio();
                if key == "language" {
                    if let Some(value) = codes.get(widget.selected() as usize) {
                        if settings.string(key).as_str() != value {
                            owner.report(settings.set_string(key, value));
                        }
                    }
                } else if settings.int(key) != widget.selected() as i32 {
                    owner.report(settings.set_int(key, widget.selected() as i32));
                }
            }
        });
        self.controls
            .borrow_mut()
            .push(Control::Choice(key, widget, values));
    }

    fn string_list(
        &self,
        group: &gtk::ListBox,
        key: &'static str,
        title: &str,
        subtitle: &str,
        placeholder: &str,
        should_expand: bool,
    ) {
        let manager = StringListManager::new(
            self.settings.gio(),
            key,
            title,
            placeholder,
            &self.error,
            should_expand,
        );
        let container = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let label = gtk::Label::new(Some(&tr(subtitle)));
        label.set_wrap(true);
        label.set_xalign(0.0);
        label.add_css_class("dim-label");
        container.append(&label);
        container.append(&manager.widget);
        container.set_margin_start(12);
        container.set_margin_end(12);
        container.set_margin_top(10);
        container.set_margin_bottom(10);
        group.append(&container);
        self.lists.borrow_mut().push(manager);
    }

    fn shortcut(
        &self,
        group: &gtk::ListBox,
        key: &'static str,
        title: &str,
        application: &gtk::Application,
    ) {
        let editor = ShortcutEditor::new(
            self.settings.gio(),
            key,
            &self.window,
            application,
            &self.error,
            &self.shortcut_status,
        );
        let subtitle = if key == "toggle-popup" {
            "Suggested shortcuts: Alt+X, Alt+C, or Alt+V"
        } else {
            ""
        };
        group.append(&controls::row(title, subtitle, &editor.widget));
        self.shortcuts.borrow_mut().push(editor);
    }

    fn reset(self: &Rc<Self>, group: &gtk::ListBox, keys: Vec<&'static str>) {
        let button = gtk::Button::with_label(&tr("Reset"));
        group.append(&controls::row("Reset shortcuts to defaults", "", &button));
        let weak = Rc::downgrade(self);
        button.connect_clicked(move |_| {
            if let Some(owner) = weak.upgrade() {
                for key in &keys {
                    if !owner.settings.gio().is_writable(key) {
                        owner
                            .error
                            .set_text(&format!("Setting {key} is not writable."));
                        return;
                    }
                    owner.settings.gio().reset(key);
                }
            }
        });
    }

    fn report(&self, result: Result<(), glib::BoolError>) {
        match result {
            Ok(()) => self.error.set_text(""),
            Err(error) => {
                self.error.set_text(&error.to_string());
                self.sync();
            }
        }
    }

    fn sync(&self) {
        self.is_syncing.set(true);
        for control in self.controls.borrow().iter() {
            control.sync(self.settings.gio());
            if let Control::Integer(key, widget) = control {
                match *key {
                    "popup-pages" => {
                        widget.set_sensitive(self.settings.gio().boolean("limit-popup-pages"))
                    }
                    "clear-history-interval" => widget
                        .set_sensitive(self.settings.gio().boolean("clear-history-on-interval")),
                    _ => (),
                }
            }
        }
        self.is_syncing.set(false);
    }

    pub fn present(&self) {
        let language = i18n::get_stored_language(self.settings.gio());
        if *self.labels_language.borrow() != language {
            if let (Some(owner), Some(application)) =
                (self.self_weak.upgrade(), self.window.application())
            {
                let shortcuts = self.shortcuts.take();
                for editor in shortcuts {
                    editor.disconnect();
                }
                let lists = self.lists.take();
                for list in lists {
                    list.disconnect();
                }
                self.controls.borrow_mut().clear();
                self.window.set_child(gtk::Widget::NONE);
                self.labels_language.replace(language);
                owner.build(&application);
                self.sync();
            }
        }
        self.window.present();
    }
    pub fn close(&self) {
        let shortcuts = self.shortcuts.borrow().clone();
        for editor in shortcuts {
            editor.finish_capture();
        }
        self.window.set_visible(false);
    }
    pub fn destroy(&self) {
        self.disconnect();
        self.window.destroy();
    }
    pub fn set_shortcut_status(&self, status: &str) {
        self.shortcut_status.set_text(status);
    }

    fn disconnect(&self) {
        if let Some(id) = self.changed.borrow_mut().take() {
            self.settings.gio().disconnect(id);
        }
        let shortcuts = self.shortcuts.borrow().clone();
        for editor in shortcuts {
            editor.disconnect();
        }
        for list in self.lists.borrow().iter() {
            list.disconnect();
        }
    }
}

impl Drop for SettingsWindow {
    fn drop(&mut self) {
        self.disconnect();
        self.window.destroy();
    }
}
