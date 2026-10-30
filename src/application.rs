use std::{cell::RefCell, rc::Rc};

use gtk::{gio, glib, prelude::*};

use crate::way_to_clip_controller::WayToClipController;

pub fn run() -> glib::ExitCode {
    let application = gtk::Application::builder()
        .application_id("org.waytoclip.WayToClip")
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let controller_owner: Rc<RefCell<Option<Rc<WayToClipController>>>> =
        Rc::new(RefCell::new(None));
    let hold = application.hold();

    let startup_owner = Rc::clone(&controller_owner);
    application.connect_startup(move |application| {
        let controller = match WayToClipController::new(application) {
            Ok(controller) => controller,
            Err(error) => {
                eprintln!("WayToClip: cannot initialize settings: {error}");
                application.quit();
                return;
            }
        };
        startup_owner.replace(Some(Rc::clone(&controller)));

        let activation_controller = Rc::downgrade(&controller);
        application.connect_activate(move |_| {
            if let Some(controller) = activation_controller.upgrade() {
                controller.activate();
            }
        });

        add_action(application, &controller, "quit", |controller| {
            controller.quit();
        });
        add_action(application, &controller, "settings", |controller| {
            controller.open_settings()
        });
        add_action(application, &controller, "toggle-popup", |controller| {
            controller.activate()
        });
        add_action(
            application,
            &controller,
            "refresh-shortcuts",
            |controller| controller.refresh_shortcuts(),
        );
        let ready =
            gio::SimpleAction::new_stateful("shortcut-capture-ready", None, &false.to_variant());
        ready.connect_change_state(|_, _| {});
        ready.connect_activate(|_, _| {});
        application.add_action(&ready);
        let capture =
            gio::SimpleAction::new("set-shortcut-capture", Some(glib::VariantTy::BOOLEAN));
        let weak_controller = Rc::downgrade(&controller);
        capture.connect_activate(move |_, parameter| {
            if let (Some(controller), Some(is_active)) = (
                weak_controller.upgrade(),
                parameter.and_then(|value| value.get::<bool>()),
            ) {
                controller.set_shortcut_capture(is_active);
            }
        });
        application.add_action(&capture);
        add_action(application, &controller, "clear-history", |controller| {
            controller.request_clear_history();
        });
        add_action(
            application,
            &controller,
            "toggle-private-mode",
            |controller| controller.toggle_private_mode(),
        );
        add_action(
            application,
            &controller,
            "reset-clear-timer",
            |controller| controller.reset_clear_timer(),
        );

        controller.start();
    });

    let command_owner = Rc::clone(&controller_owner);
    application.connect_command_line(move |application, command_line| {
        let arguments: Vec<String> = command_line
            .arguments()
            .iter()
            .skip(1)
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect();
        let action = match arguments.as_slice() {
            [] => None,
            [argument] if argument == "--quit" => Some("quit"),
            [argument] if argument == "--settings" => Some("settings"),
            [argument] if argument == "--clear-history" => Some("clear-history"),
            [argument] if argument == "--toggle-private-mode" => Some("toggle-private-mode"),
            [argument] if argument == "--reset-clear-timer" => Some("reset-clear-timer"),
            _ => {
                command_line.printerr_literal(
                    "Usage: waytoclip [--settings | --quit | --clear-history | --toggle-private-mode | --reset-clear-timer]\n",
                );
                return glib::ExitCode::FAILURE;
            }
        };

        if let Some(action) = action {
            application.activate_action(action, None);
        } else if command_owner.borrow().is_some() {
            application.activate();
        }
        glib::ExitCode::SUCCESS
    });

    let exit_code = application.run();
    if let Some(controller) = controller_owner.take() {
        controller.shutdown();
    }
    drop(hold);
    exit_code
}

fn add_action(
    application: &gtk::Application,
    controller: &Rc<WayToClipController>,
    name: &str,
    activate: fn(&Rc<WayToClipController>),
) {
    let action = gio::SimpleAction::new(name, None);
    let weak_controller = Rc::downgrade(controller);
    action.connect_activate(move |_, _| {
        if let Some(controller) = weak_controller.upgrade() {
            activate(&controller);
        }
    });
    application.add_action(&action);
}
