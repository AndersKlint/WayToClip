use gtk::prelude::*;

pub struct Popup {
    pub window: gtk::ApplicationWindow,
    pub paste: gtk::Button,
    pub copy: gtk::Button,
    pub read: gtk::Button,
    pub quit: gtk::Button,
    pub terminal: gtk::CheckButton,
    pub editor: gtk::TextView,
    target: gtk::Label,
    status: gtk::Label,
}

impl Popup {
    pub fn new(application: &gtk::Application) -> Self {
        let window = gtk::ApplicationWindow::builder()
            .application(application)
            .title("WayToClip")
            .decorated(false)
            .resizable(false)
            .default_width(380)
            .build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
        content.set_margin_top(12);
        content.set_margin_bottom(12);
        content.set_margin_start(12);
        content.set_margin_end(12);

        let target = gtk::Label::new(None);
        target.set_xalign(0.0);
        target.set_ellipsize(gtk::pango::EllipsizeMode::End);
        content.append(&target);

        let editor = gtk::TextView::builder()
            .wrap_mode(gtk::WrapMode::WordChar)
            .accepts_tab(false)
            .build();
        let scroll = gtk::ScrolledWindow::builder()
            .height_request(140)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&editor)
            .build();
        content.append(&scroll);

        let terminal = gtk::CheckButton::with_label("Terminal paste (Ctrl+Shift+V)");
        content.append(&terminal);
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let paste = gtk::Button::with_label("Paste");
        paste.add_css_class("suggested-action");
        let copy = gtk::Button::with_label("Copy only");
        let read = gtk::Button::with_label("Read clipboard");
        let quit = gtk::Button::with_label("Quit");
        for button in [&paste, &copy, &read, &quit] {
            actions.append(button);
        }
        content.append(&actions);

        let status = gtk::Label::new(None);
        status.set_wrap(true);
        status.set_xalign(0.0);
        status.set_visible(false);
        content.append(&status);
        window.set_child(Some(&content));
        Self {
            window,
            paste,
            copy,
            read,
            quit,
            terminal,
            editor,
            target,
            status,
        }
    }

    pub fn prepare(&self, target_app_id: &str, is_terminal: bool) {
        self.target.set_text(if target_app_id.is_empty() {
            "Target: unknown"
        } else {
            target_app_id
        });
        self.terminal.set_active(is_terminal);
        self.set_busy(false);
        self.set_status("");
    }

    pub fn text(&self) -> String {
        let buffer = self.editor.buffer();
        buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), true)
            .to_string()
    }

    pub fn set_busy(&self, is_busy: bool) {
        self.paste.set_sensitive(!is_busy);
        self.copy.set_sensitive(!is_busy);
        self.read.set_sensitive(!is_busy);
        self.editor.set_editable(!is_busy);
    }

    pub fn set_status(&self, message: &str) {
        self.status.set_text(message);
        self.status.set_visible(!message.is_empty());
    }
}
