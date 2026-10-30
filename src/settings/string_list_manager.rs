use std::{cell::RefCell, rc::Rc};

use gtk::{gio, glib, prelude::*};

use crate::common::i18n::translate_native as tr;

pub struct StringListManager {
    pub widget: gtk::Expander,
    settings: gio::Settings,
    key: &'static str,
    list: gtk::Box,
    error: gtk::Label,
    changed: RefCell<Option<glib::SignalHandlerId>>,
    chooser_task: Rc<RefCell<Option<glib::JoinHandle<()>>>>,
}

impl StringListManager {
    pub fn new(
        settings: &gio::Settings,
        key: &'static str,
        title: &str,
        placeholder: &str,
        error: &gtk::Label,
        should_expand: bool,
    ) -> Rc<Self> {
        let widget = gtk::Expander::new(Some(&tr(title)));
        let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let list = gtk::Box::new(gtk::Orientation::Vertical, 4);
        content.append(&list);
        let input = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let entry = gtk::Entry::new();
        entry.set_placeholder_text(Some(&tr(placeholder)));
        entry.set_hexpand(true);
        input.append(&entry);
        let (chooser, chooser_task) = application_chooser(&entry, error);
        input.append(&chooser);
        let add = gtk::Button::from_icon_name("list-add-symbolic");
        add.set_tooltip_text(Some(&tr("Add")));
        input.append(&add);
        let cancel = gtk::Button::from_icon_name("window-close-symbolic");
        cancel.set_tooltip_text(Some(&tr("Cancel")));
        input.append(&cancel);
        let weak_entry = entry.downgrade();
        cancel.connect_clicked(move |_| {
            if let Some(entry) = weak_entry.upgrade() {
                entry.set_text("");
            }
        });
        content.append(&input);
        widget.set_child(Some(&content));
        widget.set_expanded(should_expand && !settings.strv(key).is_empty());
        let manager = Rc::new(Self {
            widget,
            settings: settings.clone(),
            key,
            list,
            error: error.clone(),
            changed: RefCell::new(None),
            chooser_task,
        });
        let weak = Rc::downgrade(&manager);
        let weak_entry = entry.downgrade();
        add.connect_clicked(move |_| {
            if let (Some(manager), Some(entry)) = (weak.upgrade(), weak_entry.upgrade()) {
                manager.add(&entry);
            }
        });
        let weak = Rc::downgrade(&manager);
        entry.connect_activate(move |entry| {
            if let Some(manager) = weak.upgrade() {
                manager.add(entry);
            }
        });
        let weak = Rc::downgrade(&manager);
        let id = settings.connect_changed(Some(key), move |_, _| {
            if let Some(manager) = weak.upgrade() {
                manager.render();
            }
        });
        manager.changed.replace(Some(id));
        manager.render();
        manager
    }

    fn add(&self, entry: &gtk::Entry) {
        let value = entry.text().trim().to_string();
        if value.is_empty() {
            return;
        }
        let mut values: Vec<String> = self
            .settings
            .strv(self.key)
            .iter()
            .map(ToString::to_string)
            .collect();
        values.push(value);
        if self.write(&values) {
            entry.set_text("");
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

    fn render(self: &Rc<Self>) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        for value in self.settings.strv(self.key).iter() {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            let label = gtk::Label::new(Some(value));
            label.set_xalign(0.0);
            label.set_hexpand(true);
            label.set_wrap(true);
            row.append(&label);
            let remove = gtk::Button::from_icon_name("edit-delete-symbolic");
            remove.set_tooltip_text(Some(&tr("Remove")));
            row.append(&remove);
            let value = value.to_string();
            let weak = Rc::downgrade(self);
            remove.connect_clicked(move |_| {
                if let Some(manager) = weak.upgrade() {
                    let values: Vec<String> = manager
                        .settings
                        .strv(manager.key)
                        .iter()
                        .filter(|item| item.as_str() != value)
                        .map(ToString::to_string)
                        .collect();
                    manager.write(&values);
                }
            });
            self.list.append(&row);
        }
    }

    pub fn disconnect(&self) {
        if let Some(id) = self.changed.borrow_mut().take() {
            self.settings.disconnect(id);
        }
        if let Some(task) = self.chooser_task.borrow_mut().take() {
            task.abort();
        }
    }
}

impl Drop for StringListManager {
    fn drop(&mut self) {
        self.disconnect();
    }
}

fn application_chooser(
    entry: &gtk::Entry,
    error: &gtk::Label,
) -> (gtk::MenuButton, Rc<RefCell<Option<glib::JoinHandle<()>>>>) {
    let button = gtk::MenuButton::builder()
        .icon_name("view-list-symbolic")
        .tooltip_text(tr("Choose from installed applications"))
        .build();
    let popover = gtk::Popover::new();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some(&tr("Search applications...")));
    content.append(&search);
    let list = gtk::ListBox::new();
    let scroll = gtk::ScrolledWindow::builder()
        .min_content_width(300)
        .min_content_height(300)
        .child(&list)
        .build();
    content.append(&scroll);
    popover.set_child(Some(&content));
    button.set_popover(Some(&popover));
    let weak_list = list.downgrade();
    let weak_entry = entry.downgrade();
    let weak_popover = popover.downgrade();
    list.connect_row_activated(move |_, row| {
        if let (Some(entry), Some(popover)) = (weak_entry.upgrade(), weak_popover.upgrade()) {
            if let Some(id) = row.tooltip_text() {
                entry.set_text(&id);
                popover.popdown();
                entry.grab_focus();
            }
        }
    });
    search.connect_search_changed(move |search| {
        if let Some(list) = weak_list.upgrade() {
            let mut child = list.first_child();
            while let Some(row) = child {
                child = row.next_sibling();
                row.set_visible(
                    row.widget_name()
                        .to_lowercase()
                        .contains(&search.text().to_lowercase()),
                );
            }
        }
    });
    let weak_list = list.downgrade();
    search.connect_activate(move |_| {
        if let Some(list) = weak_list.upgrade() {
            let mut child = list.first_child();
            while let Some(row) = child {
                child = row.next_sibling();
                if row.is_visible() {
                    if let Ok(row) = row.downcast::<gtk::ListBoxRow>() {
                        list.emit_by_name::<()>("row-activated", &[&row]);
                    }
                    break;
                }
            }
        }
    });
    let task = Rc::new(RefCell::new(None));
    let callback_task = task.clone();
    let weak_error = error.downgrade();
    let weak_search = search.downgrade();
    let weak_list = list.downgrade();
    button.connect_active_notify(move |button| {
        if !button.is_active() {
            return;
        }
        let Some(list) = weak_list.upgrade() else {
            return;
        };
        if list.first_child().is_some() || !list.is_sensitive() {
            return;
        }
        list.set_sensitive(false);
        let weak_list = list.downgrade();
        let weak_error = weak_error.clone();
        let weak_search = weak_search.clone();
        let handle = glib::MainContext::default().spawn_local(async move {
            let apps = gio::spawn_blocking(|| {
                let mut apps: Vec<(String, String)> = gio::AppInfo::all()
                    .into_iter()
                    .filter(|app| app.should_show())
                    .filter_map(|app| {
                        app.id().map(|id| {
                            (
                                app.display_name().to_string(),
                                id.trim_end_matches(".desktop").to_string(),
                            )
                        })
                    })
                    .collect();
                apps.sort_by_key(|(name, _)| name.to_lowercase());
                apps
            })
            .await;
            if let Some(list) = weak_list.upgrade() {
                list.set_sensitive(true);
                match apps {
                    Ok(apps) => {
                        let query = weak_search
                            .upgrade()
                            .map(|search| search.text().to_lowercase())
                            .unwrap_or_default();
                        for (name, id) in apps {
                            let row = gtk::ListBoxRow::new();
                            row.set_widget_name(&name);
                            row.set_tooltip_text(Some(&id));
                            row.set_child(Some(&gtk::Label::new(Some(&name))));
                            row.set_visible(name.to_lowercase().contains(&query));
                            list.append(&row);
                        }
                    }
                    Err(error) => {
                        if let Some(label) = weak_error.upgrade() {
                            label.set_text(&format!(
                                "Could not list installed applications: {error:?}"
                            ));
                        }
                    }
                }
            }
        });
        callback_task.replace(Some(handle));
    });
    (button, task)
}
