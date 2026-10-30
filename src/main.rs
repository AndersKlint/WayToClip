mod application;
mod bridge;
mod clipboard {
    pub mod kde;
}
mod popup;
mod session;

fn main() -> gtk::glib::ExitCode {
    application::run()
}
