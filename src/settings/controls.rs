use gtk::{gio, prelude::*};

use crate::common::i18n::translate_native as tr;

pub enum Control {
    Boolean(&'static str, gtk::Switch),
    Integer(&'static str, gtk::SpinButton),
    Choice(&'static str, gtk::DropDown, Vec<String>),
}

impl Control {
    pub fn sync(&self, settings: &gio::Settings) {
        match self {
            Self::Boolean(key, widget) => widget.set_active(settings.boolean(key)),
            Self::Integer(key, widget) => widget.set_value(settings.int(key) as f64),
            Self::Choice(key, widget, values) => {
                let index = if *key == "language" {
                    let language = crate::common::i18n::get_stored_language(settings);
                    values
                        .iter()
                        .position(|value| *value == language)
                        .unwrap_or(0) as u32
                } else {
                    settings.int(key) as u32
                };
                widget.set_selected(index);
            }
        }
    }
}

pub fn row(title: &str, subtitle: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.set_margin_top(10);
    row.set_margin_bottom(10);
    row.set_margin_start(12);
    row.set_margin_end(12);
    let labels = gtk::Box::new(gtk::Orientation::Vertical, 4);
    labels.set_hexpand(true);
    let label = gtk::Label::new(Some(&tr(title)));
    label.set_xalign(0.0);
    label.set_wrap(true);
    labels.append(&label);
    if !subtitle.is_empty() {
        let label = gtk::Label::new(Some(&tr(subtitle)));
        label.set_xalign(0.0);
        label.set_wrap(true);
        label.add_css_class("dim-label");
        labels.append(&label);
    }
    control.set_valign(gtk::Align::Center);
    row.append(&labels);
    row.append(control);
    row
}

pub fn group(content: &gtk::Box, title: &str, description: &str) -> gtk::ListBox {
    let label = gtk::Label::new(Some(&tr(title)));
    label.set_xalign(0.0);
    label.add_css_class("heading");
    label.set_margin_top(18);
    content.append(&label);
    if !description.is_empty() {
        let label = gtk::Label::new(Some(&tr(description)));
        label.set_xalign(0.0);
        label.set_wrap(true);
        label.add_css_class("dim-label");
        content.append(&label);
    }
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("boxed-list");
    content.append(&list);
    list
}
