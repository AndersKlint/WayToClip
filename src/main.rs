mod application;
mod bridge;
mod clipboard;
mod common;
mod cursor_popup;
mod history;
mod paste;
mod session;
mod settings;
mod way_to_clip_controller;

fn main() -> gtk::glib::ExitCode {
    application::run()
}
