mod application;
mod bridge;
mod popup;
mod session;

fn main() -> gtk::glib::ExitCode {
    application::run()
}
