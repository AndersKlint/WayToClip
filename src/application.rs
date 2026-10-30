use std::{cell::RefCell, rc::Rc};

use gtk::{gio, glib, prelude::*};

use crate::session::Session;

pub fn run() -> glib::ExitCode {
    let application = gtk::Application::builder()
        .application_id("org.waytoclip.WayToClip")
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let session_owner = Rc::new(RefCell::new(None));
    let weak_owner = Rc::downgrade(&session_owner);
    let hold = application.hold();
    application.connect_startup(move |application| {
        let session = Session::new(application);
        let weak_session = Rc::downgrade(&session);
        let activation_session = weak_session.clone();
        application.connect_activate(move |_| {
            if let Some(session) = activation_session.upgrade() {
                session.activate();
            }
        });
        let quit = gio::SimpleAction::new("quit", None);
        quit.connect_activate(move |_, _| {
            if let Some(session) = weak_session.upgrade() {
                session.quit();
            }
        });
        application.add_action(&quit);
        if let Some(owner) = weak_owner.upgrade() {
            owner.replace(Some(session));
        }
    });
    application.connect_command_line(|application, command_line| {
        let arguments = command_line.arguments();
        if arguments.len() == 2 && arguments[1] == "--quit" {
            application.activate_action("quit", None);
        } else if arguments.len() == 1 {
            application.activate();
        } else {
            command_line.printerr_literal("Usage: waytoclip [--quit]\n");
            return glib::ExitCode::FAILURE;
        }
        glib::ExitCode::SUCCESS
    });
    let exit_code = application.run();
    if let Some(session) = session_owner.take() {
        session.shutdown();
    }
    drop(hold);
    exit_code
}
