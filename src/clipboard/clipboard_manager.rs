use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk::glib;

use crate::{
    bridge::{Bridge, ClipboardOffer},
    clipboard::secret_hints::{SECRET_HINT_MIMETYPES, is_secret_hint_payload},
    common::constants::CLIPBOARD_MIMETYPES,
};

const CANONICAL_TEXT_MIMETYPE: &str = "text/plain;charset=utf-8";

#[derive(Clone)]
pub struct ClipboardPolicy {
    pub is_private_mode: bool,
    pub should_ignore_secrets: bool,
    pub should_cache_images: bool,
    pub excluded_apps: Vec<String>,
}

#[derive(Clone)]
pub struct CapturedClipboard {
    pub mimetype: String,
    pub bytes: Vec<u8>,
}

type PolicyReader = dyn Fn() -> ClipboardPolicy;
type CaptureHandler = dyn Fn(CapturedClipboard);

pub struct ClipboardManager {
    policy: RefCell<Option<Rc<PolicyReader>>>,
    handler: RefCell<Option<Rc<CaptureHandler>>>,
    inhibit_count: Cell<u32>,
    privacy_generation: Cell<u64>,
    latest_serial: Cell<u64>,
    pending_read: RefCell<Option<glib::JoinHandle<()>>>,
}

impl ClipboardManager {
    pub fn new() -> Rc<Self> {
        Rc::new(Self {
            policy: RefCell::new(None),
            handler: RefCell::new(None),
            inhibit_count: Cell::new(0),
            privacy_generation: Cell::new(0),
            latest_serial: Cell::new(0),
            pending_read: RefCell::new(None),
        })
    }

    pub fn set_policy(&self, policy: Rc<PolicyReader>) {
        self.policy.replace(Some(policy));
    }

    pub fn set_handler(&self, handler: Rc<CaptureHandler>) {
        self.handler.replace(Some(handler));
    }

    pub fn invalidate_pending(&self) {
        self.privacy_generation
            .set(self.privacy_generation.get().wrapping_add(1));
        if let Some(task) = self.pending_read.borrow_mut().take() {
            task.abort();
        }
    }

    pub async fn write(
        &self,
        bridge: &Bridge,
        mimetype: &str,
        bytes: &[u8],
    ) -> Result<(), glib::Error> {
        self.inhibit_count.set(self.inhibit_count.get() + 1);
        let result = bridge.write_clipboard(mimetype, bytes).await;
        self.inhibit_count
            .set(self.inhibit_count.get().saturating_sub(1));
        result
    }

    pub async fn clear(&self, bridge: &Bridge) -> Result<(), glib::Error> {
        self.write(bridge, CANONICAL_TEXT_MIMETYPE, b"").await
    }

    pub async fn run(self: &Rc<Self>, bridge: Rc<Bridge>) -> Result<(), glib::Error> {
        let mut previous_serial: Option<u64> = None;
        loop {
            let offer = bridge
                .watch_clipboard(previous_serial.unwrap_or_default())
                .await?;
            if previous_serial == Some(offer.serial) {
                continue;
            }
            previous_serial = Some(offer.serial);
            self.latest_serial.set(offer.serial);
            if let Some(task) = self.pending_read.borrow_mut().take() {
                task.abort();
            }
            let policy = match self.policy.borrow().as_ref() {
                Some(policy) => policy(),
                None => continue,
            };
            if policy.is_private_mode
                || self.inhibit_count.get() > 0
                || offer.is_own
                || is_excluded_source(&offer, &policy.excluded_apps)
            {
                continue;
            }
            let generation = self.privacy_generation.get();
            let serial = offer.serial;
            let weak_manager = Rc::downgrade(self);
            let read_bridge = Rc::clone(&bridge);
            let task = glib::spawn_future_local(async move {
                let Some(manager) = weak_manager.upgrade() else {
                    return;
                };
                let result = read_capture(&manager, &read_bridge, &offer, &policy).await;
                if manager.privacy_generation.get() != generation
                    || manager.latest_serial.get() != serial
                {
                    return;
                }
                let should_suppress = match manager.policy.borrow().as_ref() {
                    Some(policy) => {
                        let policy = policy();
                        policy.is_private_mode
                            || is_excluded_source(&offer, &policy.excluded_apps)
                            || matches!(&result, Ok(Some(capture)) if capture.mimetype.starts_with("image/") && !policy.should_cache_images)
                    }
                    None => true,
                };
                if should_suppress {
                    return;
                }
                match result {
                    Ok(Some(capture)) => {
                        let handler = manager.handler.borrow().clone();
                        if let Some(handler) = handler {
                            handler(capture);
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        eprintln!("WayToClip: clipboard read failed: {error}");
                    }
                }
                manager.pending_read.borrow_mut().take();
            });
            self.pending_read.replace(Some(task));
        }
    }
}

async fn read_capture(
    manager: &ClipboardManager,
    bridge: &Bridge,
    offer: &ClipboardOffer,
    policy: &ClipboardPolicy,
) -> Result<Option<CapturedClipboard>, glib::Error> {
    if policy.should_ignore_secrets && has_secret_hint(bridge, offer).await {
        return Ok(None);
    }
    for wanted in CLIPBOARD_MIMETYPES {
        let Some(mimetype) = offered_mimetype(offer, wanted) else {
            continue;
        };
        let Some(policy) = manager.policy.borrow().as_ref().map(|policy| policy()) else {
            return Ok(None);
        };
        if policy.is_private_mode
            || is_excluded_source(offer, &policy.excluded_apps)
            || (wanted.starts_with("image/") && !policy.should_cache_images)
        {
            return Ok(None);
        }
        let bytes = match bridge.read_clipboard_content(offer.serial, mimetype).await {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        if bytes.is_empty() {
            continue;
        }
        return Ok(Some(CapturedClipboard {
            mimetype: canonicalize_mimetype(wanted),
            bytes,
        }));
    }
    Ok(None)
}

async fn has_secret_hint(bridge: &Bridge, offer: &ClipboardOffer) -> bool {
    for wanted in SECRET_HINT_MIMETYPES {
        let Some(mimetype) = offered_mimetype(offer, wanted) else {
            continue;
        };
        let Ok(bytes) = bridge.read_clipboard_content(offer.serial, mimetype).await else {
            continue;
        };
        if !bytes.is_empty() && is_secret_hint_payload(mimetype, &bytes) {
            return true;
        }
    }
    false
}

fn offered_mimetype<'a>(offer: &'a ClipboardOffer, wanted: &str) -> Option<&'a str> {
    offer
        .mimetypes
        .iter()
        .find(|mimetype| mimetype.eq_ignore_ascii_case(wanted))
        .map(String::as_str)
}

fn is_excluded_source(offer: &ClipboardOffer, excluded_apps: &[String]) -> bool {
    excluded_apps.iter().any(|excluded| {
        let excluded = excluded.trim();
        !excluded.is_empty()
            && ((!offer.wm_class.is_empty() && offer.wm_class == excluded)
                || (!offer.app_id.is_empty() && offer.app_id == excluded))
    })
}

fn canonicalize_mimetype(mimetype: &str) -> String {
    let lowercase = mimetype.to_ascii_lowercase();
    if lowercase == "text/plain"
        || lowercase.starts_with("text/plain;")
        || lowercase == "string"
        || lowercase == "utf8_string"
    {
        CANONICAL_TEXT_MIMETYPE.to_owned()
    } else {
        mimetype.to_owned()
    }
}
