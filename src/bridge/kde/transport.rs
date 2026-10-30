use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    rc::{Rc, Weak},
    time::{Duration, Instant},
};

use futures_channel::oneshot;
use glib::variant::ToVariant;
use gtk::{gio, glib};
use serde_json::{Value, json};

use super::{PopupTarget, error};
use crate::clipboard::kde::{FocusedApp, FocusedAppHandle};

const INTERFACE: &str = "org.waytoclip.WayToClip.Kde";
const XML: &str = r#"<node><interface name="org.waytoclip.WayToClip.Kde"><method name="NextCommand"><arg type="s" direction="in"/><arg type="s" direction="out"/></method><method name="ReportResult"><arg type="s" direction="in"/><arg type="s" direction="in"/></method><method name="ReportEvent"><arg type="s" direction="in"/><arg type="s" direction="in"/></method></interface></node>"#;

struct Pending {
    token: u32,
    reply: oneshot::Sender<Result<Value, glib::Error>>,
}

#[derive(Default)]
struct State {
    owner: Option<String>,
    instance: Option<String>,
    retired_instances: HashSet<String>,
    next_token: u32,
    next_request: u32,
    active: Option<u32>,
    active_has_target: bool,
    focused: FocusedAppHandle,
    pending: HashMap<u32, Pending>,
    queue: VecDeque<(u32, String)>,
    poll: Option<(u64, oneshot::Sender<String>)>,
    next_poll: u64,
}

impl State {
    fn invalidate(&mut self, reason: &str) {
        self.active = None;
        self.active_has_target = false;
        self.queue.clear();
        for (_, pending) in self.pending.drain() {
            let _ = pending.reply.send(Err(error(reason)));
        }
    }

    fn replace_owner(&mut self, owner: Option<String>) {
        if self.owner != owner {
            self.invalidate("KWin owner changed or vanished");
            self.instance = None;
            self.retired_instances.clear();
            if let Ok(mut focused) = self.focused.lock() {
                *focused = FocusedApp::default();
            }
            if let Some((_, poll)) = self.poll.take() {
                let _ = poll.send(json!({"operation": "idle"}).to_string());
            }
            self.owner = owner;
        }
    }
}

pub(super) fn required_i32(value: &Value, name: &str) -> Result<i32, glib::Error> {
    value
        .as_i64()
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| error(&format!("KWin begin result requires {name}")))
}

fn parse_target(token: u32, reply: &Value) -> Result<PopupTarget, glib::Error> {
    let has_target = reply["has_target"]
        .as_bool()
        .ok_or_else(|| error("KWin begin result requires has_target"))?;
    let available_width = required_i32(&reply["available_width"], "available_width")?;
    let available_height = required_i32(&reply["available_height"], "available_height")?;
    if available_width < 0 || available_height < 0 {
        return Err(error("KWin begin result has an invalid work area"));
    }
    Ok(PopupTarget {
        token,
        app_id: reply["app_id"].as_str().unwrap_or_default().to_owned(),
        wm_class: reply["wm_class"].as_str().unwrap_or_default().to_owned(),
        is_terminal: false,
        has_target,
        available_width,
        available_height,
    })
}

pub struct Transport {
    connection: gio::DBusConnection,
    registration: Option<gio::RegistrationId>,
    unwatch: Option<Box<dyn FnOnce()>>,
    state: Rc<RefCell<State>>,
}

struct RequestGuard {
    state: Weak<RefCell<State>>,
    request: u32,
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        if let Some(state) = self.state.upgrade() {
            let mut state = state.borrow_mut();
            state.pending.remove(&self.request);
            state.queue.retain(|(id, _)| *id != self.request);
        }
    }
}

impl Transport {
    pub fn new(
        connection: gio::DBusConnection,
        focused: FocusedAppHandle,
    ) -> Result<Self, glib::Error> {
        let state = Rc::new(RefCell::new(State {
            focused,
            ..State::default()
        }));
        let weak = Rc::downgrade(&state);
        let vanished = weak.clone();
        let watcher = gio::bus_watch_name_on_connection(
            &connection,
            "org.kde.KWin",
            gio::BusNameWatcherFlags::NONE,
            move |_, _, owner| {
                if let Some(state) = weak.upgrade() {
                    state.borrow_mut().replace_owner(Some(owner.to_owned()));
                }
            },
            move |_, _| {
                if let Some(state) = vanished.upgrade() {
                    state.borrow_mut().replace_owner(None);
                }
            },
        );
        let info = gio::DBusNodeInfo::for_xml(XML)?
            .lookup_interface(INTERFACE)
            .unwrap();
        let weak = Rc::downgrade(&state);
        let registration = connection
            .register_object("/org/waytoclip/WayToClip/Kde", &info)
            .method_call(
                move |connection, sender, _, _, method, parameters, invocation| {
                    let weak = weak.clone();
                    let sender = sender.map(str::to_owned);
                    let method = method.to_owned();
                    invocation.return_future_local(async move {
                        let owner = connection
                            .call_future(
                                Some("org.freedesktop.DBus"),
                                "/org/freedesktop/DBus",
                                "org.freedesktop.DBus",
                                "GetNameOwner",
                                Some(&("org.kde.KWin",).to_variant()),
                                None,
                                gio::DBusCallFlags::NONE,
                                2000,
                            )
                            .await?;
                        let (owner,) = owner
                            .get::<(String,)>()
                            .ok_or_else(|| error("Invalid KWin owner reply"))?;
                        if sender.as_deref() != Some(owner.as_str()) {
                            return Err(error(
                                "Only the org.kde.KWin owner may call this endpoint",
                            ));
                        }
                        let state = weak
                            .upgrade()
                            .ok_or_else(|| error("KDE controller shut down"))?;
                        if state.borrow().owner.as_deref() != Some(owner.as_str()) {
                            return Err(error("KWin owner changed during authorization; retry"));
                        }
                        dispatch(&state, &method, parameters).await.map(Some)
                    });
                },
            )
            .build();
        let registration = match registration {
            Ok(registration) => registration,
            Err(error) => {
                gio::bus_unwatch_name(watcher);
                return Err(error);
            }
        };
        Ok(Self {
            connection,
            registration: Some(registration),
            unwatch: Some(Box::new(move || gio::bus_unwatch_name(watcher))),
            state,
        })
    }

    pub fn validate(&self, token: u32) -> Result<(), glib::Error> {
        if self.state.borrow().active == Some(token) {
            Ok(())
        } else {
            Err(error("KDE popup session is no longer active"))
        }
    }

    pub fn has_target(&self, token: u32) -> Result<bool, glib::Error> {
        let state = self.state.borrow();
        if state.active != Some(token) {
            return Err(error("KDE popup session is no longer active"));
        }
        Ok(state.active_has_target)
    }

    pub fn clear(&self, token: u32, reason: &str) {
        let mut state = self.state.borrow_mut();
        if state.active == Some(token) {
            state.invalidate(reason);
        }
    }

    pub async fn begin(&self, popup_position_mode: i32) -> Result<PopupTarget, glib::Error> {
        let deadline = Instant::now() + Duration::from_secs(3);
        while self.state.borrow().instance.is_none() {
            if Instant::now() >= deadline {
                return Err(error(
                    "WayToClip KWin script did not register within 3 seconds; enable the script in KDE Wayland",
                ));
            }
            glib::timeout_future(Duration::from_millis(25)).await;
        }
        let token = {
            let mut state = self.state.borrow_mut();
            state.invalidate("Replaced popup session");
            state.next_token = state
                .next_token
                .checked_add(1)
                .ok_or_else(|| error("KDE session tokens exhausted; restart WayToClip"))?;
            let token = state.next_token;
            state.active = Some(token);
            token
        };
        let result = self
            .command(
                "begin",
                token,
                json!({
                    "pid": std::process::id(),
                    "popup_position_mode": popup_position_mode,
                }),
            )
            .await;
        match result {
            Ok(reply) => {
                let target = parse_target(token, &reply);
                match target {
                    Ok(target) => {
                        self.state.borrow_mut().active_has_target = target.has_target;
                        Ok(target)
                    }
                    Err(cause) => {
                        let _ = self
                            .command("cancel", token, json!({"should_restore_focus": false}))
                            .await;
                        self.clear(token, "Invalid begin result");
                        Err(cause)
                    }
                }
            }
            Err(error) => {
                if self.validate(token).is_ok() {
                    let _ = self
                        .command("cancel", token, json!({"should_restore_focus": false}))
                        .await;
                }
                self.clear(token, "Begin failed");
                Err(error)
            }
        }
    }

    pub async fn command(
        &self,
        operation: &str,
        token: u32,
        mut command: Value,
    ) -> Result<Value, glib::Error> {
        self.validate(token)?;
        let (sender, receiver) = oneshot::channel();
        let request = {
            let mut state = self.state.borrow_mut();
            state.next_request = state
                .next_request
                .checked_add(1)
                .ok_or_else(|| error("KDE request IDs exhausted; restart WayToClip"))?;
            let request = state.next_request;
            command["operation"] = json!(operation);
            command["request"] = json!(request);
            command["token"] = json!(token);
            let command = command.to_string();
            state.pending.insert(
                request,
                Pending {
                    token,
                    reply: sender,
                },
            );
            if let Some((_, poll)) = state.poll.take() {
                if let Err(command) = poll.send(command) {
                    state.queue.push_back((request, command));
                }
            } else {
                state.queue.push_back((request, command));
            }
            request
        };
        let guard = RequestGuard {
            state: Rc::downgrade(&self.state),
            request,
        };
        let result = glib::future_with_timeout(Duration::from_secs(3), receiver).await;
        drop(guard);
        match result {
            Ok(Ok(result)) => {
                self.validate(token)?;
                result
            }
            _ => Err(error("KWin command timed out or controller disconnected")),
        }
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        let mut state = self.state.borrow_mut();
        state.invalidate("KDE controller shut down");
        state.poll.take();
        if let Some(id) = self.registration.take() {
            let _ = self.connection.unregister_object(id);
        }
        if let Some(unwatch) = self.unwatch.take() {
            unwatch();
        }
    }
}

async fn dispatch(
    state: &Rc<RefCell<State>>,
    method: &str,
    parameters: glib::Variant,
) -> Result<glib::Variant, glib::Error> {
    let (instance, payload) = if method == "NextCommand" {
        let (instance,) = parameters
            .get::<(String,)>()
            .ok_or_else(|| error("Expected instance"))?;
        (instance, None)
    } else {
        let (instance, payload) = parameters
            .get::<(String, String)>()
            .ok_or_else(|| error("Expected instance and JSON"))?;
        (instance, Some(payload))
    };
    if instance.is_empty() || instance.len() > 256 {
        return Err(error("Invalid script instance"));
    }
    {
        let mut state = state.borrow_mut();
        if state.instance.as_deref() != Some(&instance) {
            if method != "NextCommand" && method != "ReportEvent" {
                return Err(error("Script instance is no longer registered"));
            }
            if state.retired_instances.contains(&instance) {
                return Err(error("Script instance has been replaced"));
            }
            if state.retired_instances.len() >= 128 {
                return Err(error(
                    "Too many KWin script replacements; restart WayToClip",
                ));
            }
            if let Some(previous) = state.instance.take() {
                state.retired_instances.insert(previous);
            }
            state.invalidate("KWin script instance replaced");
            if let Ok(mut focused) = state.focused.lock() {
                *focused = FocusedApp::default();
            }
            if let Some((_, poll)) = state.poll.take() {
                let _ = poll.send(json!({"operation":"idle"}).to_string());
            }
            state.instance = Some(instance);
        }
    }
    if method == "NextCommand" {
        let (id, receiver) = {
            let mut state = state.borrow_mut();
            if state.poll.is_some() {
                return Err(error("Only one NextCommand poll is allowed"));
            }
            if let Some((_, command)) = state.queue.pop_front() {
                return Ok((command,).to_variant());
            }
            state.next_poll = state.next_poll.wrapping_add(1);
            let id = state.next_poll;
            let (sender, receiver) = oneshot::channel();
            state.poll = Some((id, sender));
            (id, receiver)
        };
        let result = glib::future_with_timeout(Duration::from_secs(13), receiver).await;
        let mut state = state.borrow_mut();
        if state
            .poll
            .as_ref()
            .is_some_and(|(pending, _)| *pending == id)
        {
            state.poll.take();
        }
        let command = match result {
            Ok(Ok(command)) if is_pending_command(&state, &command) => command,
            _ => json!({"operation":"idle"}).to_string(),
        };
        return Ok((command,).to_variant());
    }
    let payload = payload.ok_or_else(|| error("Missing JSON"))?;
    if payload.len() > 16384 {
        return Err(error("KWin JSON exceeds limit"));
    }
    let value: Value =
        serde_json::from_str(&payload).map_err(|e| error(&format!("Invalid KWin JSON: {e}")))?;
    if method == "ReportEvent" && value["event"].as_str() == Some("active_app") {
        let app_id = value["app_id"].as_str().unwrap_or_default();
        let wm_class = value["wm_class"].as_str().unwrap_or_default();
        if app_id.len() > 256 || wm_class.len() > 256 {
            return Err(error("Invalid active application snapshot"));
        }
        let focused = FocusedApp {
            app_id: app_id.to_owned(),
            wm_class: wm_class.to_owned(),
        };
        let handle = state.borrow().focused.clone();
        if let Ok(mut snapshot) = handle.lock() {
            *snapshot = focused;
        }
        return Ok(().to_variant());
    }
    let token = value["token"]
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| error("Invalid token"))?;
    let mut state = state.borrow_mut();
    match method {
        "ReportEvent" => {
            if value["event"] != "invalidated" || !value["error"].is_string() {
                return Err(error("Invalid KWin event"));
            }
            if state.active == Some(token) {
                state.invalidate(value["error"].as_str().unwrap());
            }
        }
        "ReportResult" => {
            let request = value["request"]
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| error("Invalid request"))?;
            let is_ok = value["ok"]
                .as_bool()
                .ok_or_else(|| error("Missing result status"))?;
            if state
                .pending
                .get(&request)
                .is_some_and(|p| p.token == token)
                && state.active == Some(token)
            {
                let pending = state.pending.remove(&request).unwrap();
                let result = if is_ok {
                    Ok(value)
                } else {
                    Err(error(
                        value["error"].as_str().unwrap_or("KWin operation failed"),
                    ))
                };
                let _ = pending.reply.send(result);
            }
        }
        _ => return Err(error("Unknown KDE controller method")),
    }
    Ok(().to_variant())
}

fn is_pending_command(state: &State, command: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(command) else {
        return false;
    };
    let Some(request) = value["request"]
        .as_u64()
        .and_then(|id| u32::try_from(id).ok())
    else {
        return false;
    };
    state.pending.get(&request).is_some_and(|pending| {
        state.active == Some(pending.token)
            && value["token"].as_u64() == Some(u64::from(pending.token))
    })
}
