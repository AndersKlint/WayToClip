use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashSet},
    rc::Rc,
};

use glib::translate::IntoGlib;
use gtk::{gdk, gio, glib, prelude::*};

use super::error;

const SERVICE: &str = "org.kde.kglobalaccel";
const PATH: &str = "/kglobalaccel";
const INTERFACE: &str = "org.kde.KGlobalAccel";
const COMPONENT: &str = "org.waytoclip.WayToClip";
const ACTIONS: [&str; 3] = ["toggle-popup", "clear-history", "toggle-private-mode"];

type ShortcutInfo = (
    String,
    String,
    String,
    String,
    String,
    String,
    Vec<i32>,
    Vec<i32>,
);

#[derive(Default)]
struct Registration {
    owner: String,
    owner_revision: u64,
    actions: HashSet<String>,
    subscription: Option<gio::SignalSubscription>,
}

pub(super) struct Shortcuts {
    connection: gio::DBusConnection,
    component: String,
    registration: Rc<RefCell<Registration>>,
    is_configuring: Cell<bool>,
    owner_watch: Option<gio::SignalSubscription>,
}

struct ConfigureGuard<'a> {
    shortcuts: &'a Shortcuts,
    is_complete: bool,
}

impl Drop for ConfigureGuard<'_> {
    fn drop(&mut self) {
        if !self.is_complete {
            self.shortcuts.clear();
        }
        self.shortcuts.is_configuring.set(false);
    }
}

impl Shortcuts {
    pub(super) fn new(connection: gio::DBusConnection) -> Self {
        let registration = Rc::new(RefCell::new(Registration::default()));
        let weak = Rc::downgrade(&registration);
        let owner_watch = connection.subscribe_to_signal(
            Some("org.freedesktop.DBus"),
            Some("org.freedesktop.DBus"),
            Some("NameOwnerChanged"),
            Some("/org/freedesktop/DBus"),
            Some(SERVICE),
            gio::DBusSignalFlags::NONE,
            move |signal| {
                let Some((name, previous, current)) =
                    signal.parameters.get::<(String, String, String)>()
                else {
                    return;
                };
                if name != SERVICE || previous == current {
                    return;
                }
                let Some(registration) = weak.upgrade() else {
                    return;
                };
                {
                    let mut registration = registration.borrow_mut();
                    registration.owner_revision = registration.owner_revision.wrapping_add(1);
                    registration.subscription.take();
                }
                if let Some(application) = gio::Application::default()
                    && application.has_action("refresh-shortcuts")
                {
                    application.activate_action("refresh-shortcuts", None);
                }
            },
        );
        Self {
            component: COMPONENT.to_owned(),
            connection,
            registration,
            is_configuring: Cell::new(false),
            owner_watch: Some(owner_watch),
        }
    }

    async fn call(
        &self,
        owner: &str,
        method: &str,
        parameters: glib::Variant,
    ) -> Result<glib::Variant, glib::Error> {
        self.connection
            .call_future(
                Some(owner),
                PATH,
                INTERFACE,
                method,
                Some(&parameters),
                None,
                gio::DBusCallFlags::NO_AUTO_START,
                3000,
            )
            .await
    }

    fn clear(&self) {
        let mut registration = self.registration.borrow_mut();
        registration.subscription.take();
        let owner = registration.owner.clone();
        for action in registration.actions.drain() {
            self.connection.call(
                Some(&owner),
                PATH,
                INTERFACE,
                "unregister",
                Some(&(self.component.as_str(), action).to_variant()),
                None,
                gio::DBusCallFlags::NO_AUTO_START,
                3000,
                gio::Cancellable::NONE,
                |_| {},
            );
        }
    }

    pub(super) async fn configure(
        &self,
        bindings: &[(String, Vec<String>)],
    ) -> Result<(), glib::Error> {
        let mut keys = BTreeMap::new();
        let mut used = HashSet::new();
        for (action, accelerators) in bindings {
            if !ACTIONS.contains(&action.as_str()) || keys.contains_key(action) {
                return Err(error("Invalid or duplicate KDE shortcut action"));
            }
            let mut converted = Vec::new();
            for accelerator in accelerators {
                if accelerator.is_empty() {
                    continue;
                }
                let key = qt_key(accelerator)?;
                if !used.insert(key) {
                    return Err(error("Duplicate KDE global shortcut"));
                }
                converted.push((vec![key, 0, 0, 0],));
            }
            keys.insert(action.clone(), converted);
        }
        if self.is_configuring.replace(true) {
            return Err(error("KDE shortcut configuration is already in progress"));
        }
        let mut guard = ConfigureGuard {
            shortcuts: self,
            is_complete: false,
        };
        let owner_revision = self.registration.borrow().owner_revision;
        let (previous_owner, previous_actions) = {
            let mut registration = self.registration.borrow_mut();
            registration.subscription.take();
            (registration.owner.clone(), registration.actions.clone())
        };
        for action in previous_actions {
            let result = self
                .call(
                    &previous_owner,
                    "unregister",
                    (&self.component, &action).to_variant(),
                )
                .await;
            if let Err(error) = result
                && !error.matches(gio::DBusError::NameHasNoOwner)
                && !error.matches(gio::DBusError::ServiceUnknown)
            {
                return Err(error);
            }
            self.registration.borrow_mut().actions.remove(&action);
        }
        let reply = self
            .connection
            .call_future(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "GetNameOwner",
                Some(&(SERVICE,).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                3000,
            )
            .await;
        let reply = match reply {
            Ok(reply) => reply,
            Err(error) if used.is_empty() && error.matches(gio::DBusError::NameHasNoOwner) => {
                guard.is_complete = true;
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        if self.registration.borrow().owner_revision != owner_revision {
            return Err(error(
                "KGlobalAccel owner changed during shortcut configuration",
            ));
        }
        let (owner,) = reply
            .get::<(String,)>()
            .ok_or_else(|| error("Invalid KGlobalAccel owner reply"))?;
        self.registration.borrow_mut().owner = owner.clone();
        // KGlobalAccel does not release session actions when their registering client disconnects.
        for action in ACTIONS {
            let name = format!("_k_session:{action}");
            self.registration.borrow_mut().actions.insert(name.clone());
            self.call(&owner, "unregister", (&self.component, &name).to_variant())
                .await?;
            self.registration.borrow_mut().actions.remove(&name);
        }
        if self.registration.borrow().owner_revision != owner_revision {
            return Err(error(
                "KGlobalAccel owner changed during shortcut configuration",
            ));
        }
        if used.is_empty() {
            guard.is_complete = true;
            return Ok(());
        }
        for key in used {
            let reply = self
                .call(
                    &owner,
                    "globalShortcutAvailable",
                    ((vec![key],), "").to_variant(),
                )
                .await?;
            if reply.get::<(bool,)>() != Some((true,)) {
                return Err(error(
                    "KDE global shortcut conflicts with an existing binding",
                ));
            }
        }
        for (action, sequences) in &keys {
            if sequences.is_empty() {
                continue;
            }
            let name = format!("_k_session:{action}");
            let id = vec![
                self.component.clone(),
                name.clone(),
                "WayToClip".into(),
                action.clone(),
            ];
            // Track before awaiting so cancellation queues removal after the registration call.
            self.registration.borrow_mut().actions.insert(name);
            self.call(&owner, "doRegister", (&id,).to_variant()).await?;
            // SetPresent | NoAutoloading keeps GSettings authoritative without storing session actions.
            let reply = self
                .call(
                    &owner,
                    "setShortcutKeys",
                    (&id, sequences, 6u32).to_variant(),
                )
                .await?;
            let (actual,) = reply
                .get::<(Vec<(Vec<i32>,)>,)>()
                .ok_or_else(|| error("Invalid KGlobalAccel shortcut reply"))?;
            let actual: HashSet<_> = actual.into_iter().collect();
            if actual != sequences.iter().cloned().collect() {
                return Err(error(
                    "KDE could not register every requested global shortcut",
                ));
            }
        }
        // Newer daemons retain conflicting sequences, so a setter echo alone cannot prove ownership.
        for (action, sequences) in &keys {
            for sequence in sequences {
                for match_type in 0..=2i32 {
                    let reply = self
                        .call(
                            &owner,
                            "globalShortcutsByKey",
                            (sequence, (match_type,)).to_variant(),
                        )
                        .await?;
                    let (entries,) = reply
                        .get::<(Vec<ShortcutInfo>,)>()
                        .ok_or_else(|| error("Invalid KGlobalAccel conflict reply"))?;
                    for (name, _, component, _, _, _, _, _) in entries {
                        if component != self.component || name != format!("_k_session:{action}") {
                            return Err(error(
                                "KDE global shortcut conflicts with an existing binding",
                            ));
                        }
                    }
                }
            }
        }
        let reply = self
            .call(&owner, "getComponent", (&self.component,).to_variant())
            .await?;
        let (path,) = reply
            .get::<(glib::variant::ObjectPath,)>()
            .ok_or_else(|| error("Invalid KGlobalAccel component path"))?;
        if self.registration.borrow().owner_revision != owner_revision {
            return Err(error(
                "KGlobalAccel owner changed during shortcut configuration",
            ));
        }
        let weak = Rc::downgrade(&self.registration);
        let component = self.component.clone();
        let subscription = self.connection.subscribe_to_signal(
            Some(&owner),
            Some("org.kde.kglobalaccel.Component"),
            Some("globalShortcutPressed"),
            Some(path.as_str()),
            Some(&self.component),
            gio::DBusSignalFlags::NONE,
            move |signal| {
                let Some((sender_component, action, _)) =
                    signal.parameters.get::<(String, String, i64)>()
                else {
                    return;
                };
                let Some(registration) = weak.upgrade() else {
                    return;
                };
                let is_registered = {
                    let registration = registration.borrow();
                    sender_component == component
                        && signal.sender_name == registration.owner
                        && registration.owner_revision == owner_revision
                        && registration.actions.contains(&action)
                };
                if is_registered
                    && let Some(action) = action.strip_prefix("_k_session:")
                    && ACTIONS.contains(&action)
                    && let Some(application) = gio::Application::default()
                {
                    application.activate_action(action, None);
                }
            },
        );
        self.registration.borrow_mut().subscription = Some(subscription);
        guard.is_complete = true;
        Ok(())
    }
}

impl Drop for Shortcuts {
    fn drop(&mut self) {
        self.owner_watch.take();
        self.clear();
    }
}

fn qt_key(accelerator: &str) -> Result<i32, glib::Error> {
    let invalid = || error(&format!("Unsupported KDE global shortcut: {accelerator}"));
    let (key, modifiers) = gtk::accelerator_parse(accelerator).ok_or_else(invalid)?;
    let is_backtab = key == gdk::Key::ISO_Left_Tab;
    if key >= gdk::Key::KP_Space && key <= gdk::Key::KP_Equal && key != gdk::Key::KP_Enter {
        return Err(invalid());
    }
    let supported = gdk::ModifierType::SHIFT_MASK
        | gdk::ModifierType::CONTROL_MASK
        | gdk::ModifierType::ALT_MASK
        | gdk::ModifierType::SUPER_MASK
        | gdk::ModifierType::META_MASK;
    if !(modifiers & !supported).is_empty() {
        return Err(invalid());
    }
    let key = match key {
        gdk::Key::Escape => 0x01000000,
        gdk::Key::Tab => 0x01000001,
        gdk::Key::ISO_Left_Tab => 0x01000001,
        gdk::Key::BackSpace => 0x01000003,
        gdk::Key::Return => 0x01000004,
        gdk::Key::KP_Enter => 0x01000005,
        gdk::Key::Insert => 0x01000006,
        gdk::Key::Delete => 0x01000007,
        gdk::Key::Pause => 0x01000008,
        gdk::Key::Print => 0x01000009,
        gdk::Key::Home => 0x01000010,
        gdk::Key::End => 0x01000011,
        gdk::Key::Left => 0x01000012,
        gdk::Key::Up => 0x01000013,
        gdk::Key::Right => 0x01000014,
        gdk::Key::Down => 0x01000015,
        gdk::Key::Page_Up => 0x01000016,
        gdk::Key::Page_Down => 0x01000017,
        key if key >= gdk::Key::F1 && key <= gdk::Key::F35 => {
            0x01000030 + (key.into_glib() - gdk::Key::F1.into_glib()) as i32
        }
        key => {
            let character = key
                .to_unicode()
                .filter(|character| !character.is_control())
                .ok_or_else(invalid)?;
            let mut uppercase = character.to_uppercase();
            let character = uppercase.next().ok_or_else(invalid)?;
            if uppercase.next().is_some() {
                return Err(invalid());
            }
            character as i32
        }
    };
    let mut combined = if is_backtab { key | 0x02000000 } else { key };
    for (mask, qt) in [
        (gdk::ModifierType::SHIFT_MASK, 0x02000000),
        (gdk::ModifierType::CONTROL_MASK, 0x04000000),
        (gdk::ModifierType::ALT_MASK, 0x08000000),
        (
            gdk::ModifierType::SUPER_MASK | gdk::ModifierType::META_MASK,
            0x10000000,
        ),
    ] {
        if modifiers.intersects(mask) {
            combined |= qt;
        }
    }
    Ok(combined)
}
