use std::{cell::RefCell, rc::Rc, time::Duration};

use gtk::{glib, prelude::*};

#[derive(Default)]
pub struct ShortcutHighlighter {
    source: Rc<RefCell<Option<glib::SourceId>>>,
    button: RefCell<Option<glib::WeakRef<gtk::Button>>>,
}

impl ShortcutHighlighter {
    pub fn highlight(&self, button: &gtk::Button) {
        self.clear();
        button.add_css_class("suggested-action");
        button.grab_focus();
        self.button.replace(Some(button.downgrade()));
        let weak_button = button.downgrade();
        let weak_source = Rc::downgrade(&self.source);
        let source = glib::timeout_add_local_once(Duration::from_secs(4), move || {
            if let Some(source) = weak_source.upgrade() {
                source.borrow_mut().take();
            }
            if let Some(button) = weak_button.upgrade() {
                button.remove_css_class("suggested-action");
            }
        });
        self.source.replace(Some(source));
    }

    pub fn clear(&self) {
        if let Some(source) = self.source.borrow_mut().take() {
            source.remove();
        }
        if let Some(button) = self
            .button
            .borrow_mut()
            .take()
            .and_then(|button| button.upgrade())
        {
            button.remove_css_class("suggested-action");
        }
    }
}

impl Drop for ShortcutHighlighter {
    fn drop(&mut self) {
        self.clear();
    }
}
