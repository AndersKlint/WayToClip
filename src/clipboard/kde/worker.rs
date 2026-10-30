#[path = "keys.rs"]
mod keys;

use std::{
    collections::HashMap,
    fs::File,
    io::{self, Read, Write},
    os::{
        fd::{AsFd, AsRawFd, FromRawFd, OwnedFd},
        unix::{ffi::OsStrExt, fs::FileExt, net::UnixStream},
    },
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

use futures_channel::oneshot;
use gtk::{gio, glib};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum,
    backend::ObjectId,
    protocol::{wl_callback, wl_keyboard, wl_registry, wl_seat},
};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1::{self, ExtDataControlDeviceV1},
    ext_data_control_manager_v1::ExtDataControlManagerV1,
    ext_data_control_offer_v1::{self, ExtDataControlOfferV1},
    ext_data_control_source_v1::{self, ExtDataControlSourceV1},
};
use wayland_protocols_plasma::fake_input::client::org_kde_kwin_fake_input::OrgKdeKwinFakeInput;
use xkbcommon::xkb;

use super::{FocusedAppHandle, OfferSnapshot, error, io_error, stopped};

pub(super) const MAX_BYTES: usize = 16 * 1024 * 1024;
pub(super) const MAX_MIME_BYTES: usize = 4096;
const MAX_MIMES: usize = 256;
const TIMEOUT: Duration = Duration::from_secs(10);
const WATCH_TIMEOUT: Duration = Duration::from_secs(10);
const TEXT_MIMES: [&str; 3] = ["text/plain;charset=utf-8", "text/plain", "UTF8_STRING"];
type Reply<T> = oneshot::Sender<Result<T, glib::Error>>;

pub(super) enum Command {
    Watch(u64, Reply<OfferSnapshot>),
    Read(u64, String, Reply<Vec<u8>>),
    Write(String, Vec<u8>, Reply<()>),
    Paste(bool, Reply<()>),
    Shutdown,
}

struct Barrier {
    reply: Reply<()>,
    deadline: Instant,
}

struct Watch {
    after_serial: u64,
    reply: Reply<OfferSnapshot>,
    deadline: Instant,
}

struct Incoming {
    file: File,
    bytes: Vec<u8>,
    reply: Reply<Vec<u8>>,
    serial: u64,
    mimetype: String,
    deadline: Instant,
}

struct Outgoing {
    file: File,
    bytes: Arc<Vec<u8>>,
    offset: usize,
    deadline: Instant,
}

#[derive(Default)]
struct State {
    globals: HashMap<u32, (String, u32)>,
    offers: HashMap<ObjectId, (ExtDataControlOfferV1, Vec<String>)>,
    selection: Option<ExtDataControlOfferV1>,
    last_selection: Option<ObjectId>,
    sources: Vec<ExtDataControlSourceV1>,
    current: Option<OfferSnapshot>,
    serial: u64,
    has_pending_own_selection: bool,
    own_selection_deadline: Option<Instant>,
    focused: FocusedAppHandle,
    watches: Vec<Watch>,
    incoming: Vec<Incoming>,
    outgoing: Vec<Outgoing>,
    barriers: HashMap<u64, Barrier>,
    next_sync: u64,
    completed_sync: u64,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    keymap: Option<xkb::Keymap>,
    keymap_error: Option<String>,
    failure: Option<String>,
    device_name: u32,
    seat_name: u32,
    fake_name: u32,
}

pub(super) fn run(
    commands: mpsc::Receiver<Command>,
    mut wakeup: UnixStream,
    ready: Reply<()>,
    focused: FocusedAppHandle,
) {
    let result = initialize(&commands, &mut wakeup, focused);
    let (connection, mut queue, mut state, manager, device, fake) = match result {
        Ok(worker) => worker,
        Err(cause) => {
            let _ = ready.send(Err(cause));
            return;
        }
    };
    if ready.send(Ok(())).is_err() {
        return;
    }
    let result = (|| {
        loop {
            pump(&connection, &mut queue, &mut state, &mut wakeup)?;
            while let Ok(command) = commands.try_recv() {
                match command {
                    Command::Shutdown => return Ok(()),
                    Command::Watch(after_serial, reply) => {
                        if !reply.is_canceled() {
                            start_watch(&mut state, after_serial, reply);
                        }
                    }
                    Command::Read(serial, mimetype, reply) => {
                        if !reply.is_canceled() {
                            start_read(&mut state, serial, mimetype, reply);
                        }
                    }
                    Command::Write(mimetype, bytes, reply) => {
                        if reply.is_canceled() {
                            continue;
                        }
                        if state.sources.len() >= 16 || state.barriers.len() >= 16 {
                            let _ = reply.send(Err(error(
                                gio::IOErrorEnum::Busy,
                                "Too many pending clipboard publications",
                            )));
                            continue;
                        }
                        let source = manager.create_data_source(&queue.handle(), Arc::new(bytes));
                        for mime in offered_mimetypes(&mimetype) {
                            source.offer(mime);
                        }
                        device.set_selection(Some(&source));
                        state.sources.push(source);
                        state.has_pending_own_selection = true;
                        state.own_selection_deadline =
                            Some(Instant::now() + Duration::from_secs(2));
                        barrier(&connection, &queue.handle(), &mut state, reply);
                    }
                    Command::Paste(is_terminal, reply) => {
                        if reply.is_canceled() {
                            continue;
                        }
                        if state.barriers.len() >= 16 {
                            let _ = reply.send(Err(error(
                                gio::IOErrorEnum::Busy,
                                "Too many pending Wayland operations",
                            )));
                            continue;
                        }
                        let chord = state.keymap.as_ref().ok_or_else(|| {
                            state.keymap_error.clone().unwrap_or_else(|| "Paste unavailable: compositor has not supplied a keyboard keymap".into())
                        }).and_then(|keymap| keys::chord(keymap, is_terminal));
                        match chord {
                            Ok(keys) => {
                                // An unfocused connection cannot establish the target's live modifier state.
                                for key in &keys {
                                    fake.keyboard_key(*key, 1);
                                }
                                for key in keys.iter().rev() {
                                    fake.keyboard_key(*key, 0);
                                }
                                barrier(&connection, &queue.handle(), &mut state, reply);
                            }
                            Err(cause) => {
                                let _ =
                                    reply.send(Err(error(gio::IOErrorEnum::NotSupported, &cause)));
                            }
                        }
                    }
                }
            }
        }
    })();
    let message = result
        .err()
        .map(|cause: glib::Error| cause.to_string())
        .unwrap_or_else(|| "KDE Wayland worker shut down".into());
    for (_, pending) in state.barriers.drain() {
        let _ = pending
            .reply
            .send(Err(error(gio::IOErrorEnum::Closed, &message)));
    }
    for pending in state.incoming.drain(..) {
        let _ = pending
            .reply
            .send(Err(error(gio::IOErrorEnum::Closed, &message)));
    }
    for pending in state.watches.drain(..) {
        let _ = pending
            .reply
            .send(Err(error(gio::IOErrorEnum::Closed, &message)));
    }
    if fake.version() >= 5 {
        fake.destroy();
    }
    let _ = connection.flush();
}

type Initialized = (
    Connection,
    EventQueue<State>,
    State,
    ExtDataControlManagerV1,
    ExtDataControlDeviceV1,
    OrgKdeKwinFakeInput,
);

fn initialize(
    commands: &mpsc::Receiver<Command>,
    wakeup: &mut UnixStream,
    focused: FocusedAppHandle,
) -> Result<Initialized, glib::Error> {
    let connection = connect_socket(wakeup)?;
    let mut queue = connection.new_event_queue::<State>();
    let mut state = State::default();
    state.focused = focused;
    let handle = queue.handle();
    let registry = connection.display().get_registry(&handle, ());
    synchronize(&connection, &mut queue, &mut state, wakeup, commands)?;
    let find = |name: &str, minimum| {
        let matches: Vec<_> = state
            .globals
            .iter()
            .filter(|(_, (interface, version))| interface == name && *version >= minimum)
            .collect();
        if matches.len() != 1 {
            return Err(error(
                gio::IOErrorEnum::NotSupported,
                &format!(
                    "Expected one {name} global (version >= {minimum}); found {}",
                    matches.len()
                ),
            ));
        }
        Ok((*matches[0].0, matches[0].1.1))
    };
    let (manager_name, _) = find("ext_data_control_manager_v1", 1).map_err(|_| {
        error(
            gio::IOErrorEnum::NotSupported,
            "KDE clipboard requires ext_data_control_manager_v1 (KWin 6.4 or newer)",
        )
    })?;
    let (seat_name, seat_version) = find("wl_seat", 1)?;
    let (fake_name, fake_version) = find("org_kde_kwin_fake_input", 4).map_err(|_| error(gio::IOErrorEnum::PermissionDenied, "KWin fake-input v4+ is unavailable or restricted. A matching Exec desktop entry must declare X-KDE-Wayland-Interfaces=org_kde_kwin_fake_input (comma-separated, no trailing semicolon), then refresh with kbuildsycoca6 --noincremental or log out and back in."))?;
    state.device_name = manager_name;
    state.seat_name = seat_name;
    state.fake_name = fake_name;
    let manager: ExtDataControlManagerV1 = registry.bind(manager_name, 1, &handle, ());
    let seat: wl_seat::WlSeat = registry.bind(seat_name, seat_version.min(7), &handle, ());
    let fake: OrgKdeKwinFakeInput = registry.bind(fake_name, fake_version.min(5), &handle, ());
    let device = manager.get_data_device(&seat, &handle, ());
    fake.authenticate(
        "WayToClip".into(),
        "Paste edited clipboard text into the selected target".into(),
    );
    synchronize(&connection, &mut queue, &mut state, wakeup, commands)?;
    synchronize(&connection, &mut queue, &mut state, wakeup, commands)?;
    Ok((connection, queue, state, manager, device, fake))
}

fn synchronize(
    connection: &Connection,
    queue: &mut EventQueue<State>,
    state: &mut State,
    wakeup: &mut UnixStream,
    commands: &mpsc::Receiver<Command>,
) -> Result<(), glib::Error> {
    state.next_sync += 1;
    let id = state.next_sync;
    connection.display().sync(&queue.handle(), id);
    let deadline = Instant::now() + TIMEOUT;
    while state.completed_sync < id {
        if Instant::now() >= deadline {
            return Err(error(
                gio::IOErrorEnum::TimedOut,
                "KDE Wayland initialization timed out",
            ));
        }
        match commands.try_recv() {
            Ok(Command::Shutdown) | Err(mpsc::TryRecvError::Disconnected) => return Err(stopped()),
            _ => {}
        }
        pump(connection, queue, state, wakeup)?;
    }
    Ok(())
}

fn barrier(
    connection: &Connection,
    handle: &QueueHandle<State>,
    state: &mut State,
    reply: Reply<()>,
) {
    state.next_sync += 1;
    state.barriers.insert(
        state.next_sync,
        Barrier {
            reply,
            deadline: Instant::now() + TIMEOUT,
        },
    );
    connection.display().sync(handle, state.next_sync);
}

fn pump(
    connection: &Connection,
    queue: &mut EventQueue<State>,
    state: &mut State,
    wakeup: &mut UnixStream,
) -> Result<(), glib::Error> {
    queue.dispatch_pending(state).map_err(io_error)?;
    if let Some(failure) = state.failure.take() {
        return Err(io_error(failure));
    }
    service_transfers(state);
    let needs_write = match connection.flush() {
        Ok(()) => false,
        Err(wayland_client::backend::WaylandError::Io(cause))
            if cause.kind() == io::ErrorKind::WouldBlock =>
        {
            true
        }
        Err(cause) => return Err(io_error(cause)),
    };
    let Some(guard) = queue.prepare_read() else {
        return Ok(());
    };
    let mut descriptors = [
        libc::pollfd {
            fd: connection.as_fd().as_raw_fd(),
            events: libc::POLLIN | if needs_write { libc::POLLOUT } else { 0 },
            revents: 0,
        },
        libc::pollfd {
            fd: wakeup.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    let result = unsafe {
        libc::poll(
            descriptors.as_mut_ptr(),
            descriptors.len() as libc::nfds_t,
            20,
        )
    };
    if result < 0 {
        let cause = io::Error::last_os_error();
        if cause.kind() != io::ErrorKind::Interrupted {
            return Err(io_error(cause));
        }
    }
    if descriptors[0].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
        match guard.read() {
            Ok(_) => {}
            Err(wayland_client::backend::WaylandError::Io(cause))
                if cause.kind() == io::ErrorKind::WouldBlock => {}
            Err(cause) => return Err(io_error(cause)),
        }
    } else {
        drop(guard);
    }
    let mut bytes = [0; 256];
    loop {
        match wakeup.read(&mut bytes) {
            Ok(0) => return Err(stopped()),
            Ok(_) => {}
            Err(cause) if cause.kind() == io::ErrorKind::WouldBlock => break,
            Err(cause) if cause.kind() == io::ErrorKind::Interrupted => continue,
            Err(cause) => return Err(io_error(cause)),
        }
    }
    Ok(())
}

fn connect_socket(wakeup: &UnixStream) -> Result<Connection, glib::Error> {
    let display = std::env::var_os("WAYLAND_DISPLAY").ok_or_else(|| {
        io_error("WAYLAND_DISPLAY is required for a separate KDE Wayland connection")
    })?;
    let mut path = std::path::PathBuf::from(display);
    if !path.is_absolute() {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .ok_or_else(|| io_error("XDG_RUNTIME_DIR is unset"))?;
        let runtime = std::path::PathBuf::from(runtime);
        if !runtime.is_absolute() {
            return Err(io_error("XDG_RUNTIME_DIR must be absolute"));
        }
        path = runtime.join(path);
    }
    let bytes = path.as_os_str().as_bytes();
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.contains(&0) || bytes.len() >= address.sun_path.len() {
        return Err(io_error("Invalid Wayland socket path"));
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (target, byte) in address.sun_path.iter_mut().zip(bytes) {
        *target = *byte as libc::c_char;
    }
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let raw = unsafe {
            libc::socket(
                libc::AF_UNIX,
                libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
                0,
            )
        };
        if raw < 0 {
            return Err(io_error(io::Error::last_os_error()));
        }
        let socket = unsafe { OwnedFd::from_raw_fd(raw) };
        let connected = unsafe {
            libc::connect(
                socket.as_raw_fd(),
                (&address as *const libc::sockaddr_un).cast(),
                std::mem::size_of_val(&address) as libc::socklen_t,
            )
        };
        if connected == 0 {
            return Connection::from_socket(UnixStream::from(socket)).map_err(io_error);
        }
        let cause = io::Error::last_os_error();
        if !matches!(cause.raw_os_error(), Some(libc::EAGAIN) | Some(libc::EINTR)) {
            return Err(io_error(format!(
                "Cannot connect to Wayland socket {}: {cause}",
                path.display()
            )));
        }
        if Instant::now() >= deadline {
            return Err(error(
                gio::IOErrorEnum::TimedOut,
                "Connecting to KDE Wayland timed out",
            ));
        }
        let mut descriptor = libc::pollfd {
            fd: wakeup.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let result = unsafe { libc::poll(&mut descriptor, 1, 20) };
        if result > 0 {
            return Err(stopped());
        }
        if result < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return Err(io_error(io::Error::last_os_error()));
        }
    }
}

fn nonblocking(fd: &impl AsRawFd) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn is_plain_text_mime(mimetype: &str) -> bool {
    mimetype == "text/plain"
        || mimetype.starts_with("text/plain;")
        || mimetype == "STRING"
        || mimetype == "UTF8_STRING"
}

fn offered_mimetypes(mimetype: &str) -> Vec<String> {
    let mut mimes = vec![mimetype.to_owned()];
    if is_plain_text_mime(mimetype) {
        for mime in TEXT_MIMES {
            if !mimes.iter().any(|existing| existing == mime) {
                mimes.push(mime.to_owned());
            }
        }
    }
    mimes
}

fn start_watch(state: &mut State, after_serial: u64, reply: Reply<OfferSnapshot>) {
    if state.watches.len() >= 8 {
        let _ = reply.send(Err(error(
            gio::IOErrorEnum::Busy,
            "Too many pending clipboard watches",
        )));
        return;
    }
    refresh_current_mimetypes(state);
    if let Some(current) = state.current.as_ref() {
        if current.serial != after_serial
            && (!current.mimetypes.is_empty() || state.selection.is_none())
        {
            let _ = reply.send(Ok(current.clone()));
            return;
        }
    }
    state.watches.push(Watch {
        after_serial,
        reply,
        deadline: Instant::now() + WATCH_TIMEOUT,
    });
}

fn capture_current(state: &mut State, is_own: bool) {
    let Some(offer) = state.selection.as_ref() else {
        return;
    };
    let mimetypes = state
        .offers
        .get(&offer.id())
        .map(|(_, formats)| formats.clone())
        .unwrap_or_default();
    // Data-control exposes no clipboard owner; attribute the selection to the focused app snapshot.
    let focused = state
        .focused
        .lock()
        .map(|focused| focused.clone())
        .unwrap_or_default();
    state.current = Some(OfferSnapshot {
        serial: state.serial,
        mimetypes,
        app_id: focused.app_id,
        wm_class: focused.wm_class,
        is_own,
    });
}

fn refresh_current_mimetypes(state: &mut State) {
    let Some(current) = state.current.as_mut() else {
        return;
    };
    if !current.mimetypes.is_empty() {
        return;
    }
    let Some(offer) = state.selection.as_ref() else {
        return;
    };
    if let Some((_, formats)) = state.offers.get(&offer.id()) {
        current.mimetypes = formats.clone();
    }
}

fn missing_snapshot(serial: u64) -> OfferSnapshot {
    OfferSnapshot {
        serial,
        mimetypes: Vec::new(),
        app_id: String::new(),
        wm_class: String::new(),
        is_own: false,
    }
}

fn service_watches(state: &mut State, now: Instant) {
    if state
        .own_selection_deadline
        .is_some_and(|deadline| now >= deadline)
    {
        state.has_pending_own_selection = false;
        state.own_selection_deadline = None;
    }
    refresh_current_mimetypes(state);
    let mut index = 0;
    while index < state.watches.len() {
        let watch = &state.watches[index];
        if watch.reply.is_canceled() {
            state.watches.swap_remove(index);
            continue;
        }
        let has_fresh = state.current.as_ref().is_some_and(|current| {
            current.serial != watch.after_serial
                && (!current.mimetypes.is_empty() || state.selection.is_none())
        });
        if has_fresh {
            let current = state.current.as_ref().unwrap().clone();
            let watch = state.watches.swap_remove(index);
            let _ = watch.reply.send(Ok(current));
            continue;
        }
        if now >= watch.deadline {
            let result = if state.selection.is_some() {
                state
                    .current
                    .clone()
                    .unwrap_or_else(|| missing_snapshot(state.serial))
            } else {
                missing_snapshot(state.serial)
            };
            let watch = state.watches.swap_remove(index);
            let _ = watch.reply.send(Ok(result));
            continue;
        }
        index += 1;
    }
}

fn prepare_read(state: &State, serial: u64, mimetype: &str) -> Result<(File, String), glib::Error> {
    if state.incoming.len() >= 4 {
        return Err(error(
            gio::IOErrorEnum::Busy,
            "Too many pending clipboard reads",
        ));
    }
    if serial == 0 || serial != state.serial {
        return Err(error(
            gio::IOErrorEnum::Failed,
            "Clipboard selection changed before read",
        ));
    }
    let offer = state
        .selection
        .as_ref()
        .ok_or_else(|| error(gio::IOErrorEnum::NotFound, "Clipboard has no selection"))?;
    let (_, formats) = state
        .offers
        .get(&offer.id())
        .ok_or_else(|| io_error("Clipboard selection metadata is unavailable"))?;
    let offered = formats
        .iter()
        .find(|format| format.eq_ignore_ascii_case(mimetype))
        .cloned()
        .ok_or_else(|| {
            error(
                gio::IOErrorEnum::NotSupported,
                "Clipboard selection does not offer the requested MIME type",
            )
        })?;
    let mut descriptors = [-1; 2];
    if unsafe { libc::pipe2(descriptors.as_mut_ptr(), libc::O_CLOEXEC) } < 0 {
        return Err(io_error(io::Error::last_os_error()));
    }
    let reader = unsafe { File::from_raw_fd(descriptors[0]) };
    let writer = unsafe { OwnedFd::from_raw_fd(descriptors[1]) };
    nonblocking(&reader).map_err(io_error)?;
    offer.receive(offered.clone(), std::os::fd::AsFd::as_fd(&writer));
    Ok((reader, offered))
}

fn start_read(state: &mut State, serial: u64, mimetype: String, reply: Reply<Vec<u8>>) {
    match prepare_read(state, serial, &mimetype) {
        Ok((file, offered)) => state.incoming.push(Incoming {
            file,
            bytes: Vec::new(),
            reply,
            serial,
            mimetype: offered,
            deadline: Instant::now() + TIMEOUT,
        }),
        Err(cause) => {
            let _ = reply.send(Err(cause));
        }
    }
}

fn is_read_current(state: &State, serial: u64, mimetype: &str) -> bool {
    if state.serial != serial {
        return false;
    }
    let Some(offer) = state.selection.as_ref() else {
        return false;
    };
    state.offers.get(&offer.id()).is_some_and(|(_, formats)| {
        formats
            .iter()
            .any(|format| format.eq_ignore_ascii_case(mimetype))
    })
}

fn service_transfers(state: &mut State) {
    let now = Instant::now();
    let mut index = 0;
    while index < state.incoming.len() {
        let transfer = &mut state.incoming[index];
        if transfer.reply.is_canceled() {
            state.incoming.swap_remove(index);
            continue;
        }
        let mut buffer = [0; 65536];
        let result = if now >= transfer.deadline {
            Some(Err(error(
                gio::IOErrorEnum::TimedOut,
                "Clipboard read timed out",
            )))
        } else {
            match transfer.file.read(&mut buffer) {
                Ok(0) => Some(Ok(std::mem::take(&mut transfer.bytes))),
                Ok(count) if transfer.bytes.len() + count <= MAX_BYTES => {
                    transfer.bytes.extend_from_slice(&buffer[..count]);
                    None
                }
                Ok(_) => Some(Err(error(
                    gio::IOErrorEnum::NoSpace,
                    "Clipboard payload exceeds 16 MiB",
                ))),
                Err(cause)
                    if matches!(
                        cause.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    None
                }
                Err(cause) => Some(Err(io_error(cause))),
            }
        };
        if let Some(result) = result {
            let transfer = state.incoming.swap_remove(index);
            let result = match result {
                Ok(bytes) if is_read_current(state, transfer.serial, &transfer.mimetype) => {
                    Ok(bytes)
                }
                Ok(_) => Err(error(
                    gio::IOErrorEnum::Failed,
                    "Clipboard selection changed during read",
                )),
                Err(cause) => Err(cause),
            };
            let _ = transfer.reply.send(result);
        } else {
            index += 1;
        }
    }
    state.outgoing.retain_mut(|transfer| {
        if now >= transfer.deadline {
            return false;
        }
        let end = (transfer.offset + 65536).min(transfer.bytes.len());
        match transfer.file.write(&transfer.bytes[transfer.offset..end]) {
            Ok(0) => false,
            Ok(count) => {
                transfer.offset += count;
                transfer.offset < transfer.bytes.len()
            }
            Err(cause) => matches!(
                cause.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
            ),
        }
    });
    service_watches(state, now);
    let expired: Vec<_> = state
        .barriers
        .iter()
        .filter(|(_, barrier)| now >= barrier.deadline || barrier.reply.is_canceled())
        .map(|(id, _)| *id)
        .collect();
    for id in expired {
        if let Some(barrier) = state.barriers.remove(&id) {
            let _ = barrier.reply.send(Err(error(
                gio::IOErrorEnum::TimedOut,
                "KDE Wayland synchronization timed out; operation outcome is unknown",
            )));
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        _: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => {
                state.globals.insert(name, (interface, version));
            }
            wl_registry::Event::GlobalRemove { name } => {
                state.globals.remove(&name);
                if [state.device_name, state.seat_name, state.fake_name].contains(&name) {
                    state.failure = Some("Required KDE Wayland global was removed".into());
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_callback::WlCallback, u64> for State {
    fn event(
        state: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        id: &u64,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        state.completed_sync = state.completed_sync.max(*id);
        if let Some(barrier) = state.barriers.remove(id) {
            let _ = barrier.reply.send(Ok(()));
        }
    }
}

impl Dispatch<ExtDataControlDeviceV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ExtDataControlDeviceV1,
        event: ext_data_control_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_data_control_device_v1::Event::DataOffer { id } => {
                state.offers.insert(id.id(), (id, Vec::new()));
            }
            ext_data_control_device_v1::Event::Selection { id } => {
                let selected = id.as_ref().map(Proxy::id);
                let is_same = state.last_selection.as_ref() == selected.as_ref();
                // Offers carry no owner; treat the next selection echo after our publication as own.
                let is_own = state.has_pending_own_selection
                    && state
                        .own_selection_deadline
                        .is_none_or(|deadline| Instant::now() <= deadline);
                let had_selection = state.selection.is_some();
                state.has_pending_own_selection = false;
                state.own_selection_deadline = None;
                state.selection = id;
                if selected.is_some() {
                    if !is_same || is_own || state.current.is_none() {
                        state.serial = state.serial.saturating_add(1);
                        state.last_selection = selected.clone();
                        capture_current(state, is_own);
                    }
                } else {
                    state.last_selection = None;
                    if had_selection {
                        state.serial = state.serial.saturating_add(1);
                        state.current = Some(missing_snapshot(state.serial));
                    }
                }
                state.offers.retain(|id, (offer, _)| {
                    if Some(id) == selected.as_ref() {
                        true
                    } else {
                        offer.destroy();
                        false
                    }
                });
            }
            ext_data_control_device_v1::Event::PrimarySelection { id: Some(offer) } => {
                if state.selection.as_ref() != Some(&offer) {
                    state.offers.remove(&offer.id());
                    offer.destroy();
                }
            }
            ext_data_control_device_v1::Event::Finished => {
                state.failure = Some("KWin clipboard data-control device finished".into());
            }
            _ => {}
        }
    }
    wayland_client::event_created_child!(State, ExtDataControlDeviceV1, [0 => (ExtDataControlOfferV1, ())]);
}

impl Dispatch<ExtDataControlOfferV1, ()> for State {
    fn event(
        state: &mut Self,
        offer: &ExtDataControlOfferV1,
        event: ext_data_control_offer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_data_control_offer_v1::Event::Offer { mime_type } = event {
            if let Some((_, formats)) = state.offers.get_mut(&offer.id()) {
                if formats.len() < MAX_MIMES && mime_type.len() <= MAX_MIME_BYTES {
                    formats.push(mime_type);
                }
            }
        }
    }
}

impl Dispatch<ExtDataControlSourceV1, Arc<Vec<u8>>> for State {
    fn event(
        state: &mut Self,
        source: &ExtDataControlSourceV1,
        event: ext_data_control_source_v1::Event,
        bytes: &Arc<Vec<u8>>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_data_control_source_v1::Event::Send { mime_type, fd } => {
                let file = File::from(fd);
                if state.outgoing.len() < 16
                    && mime_type.len() <= MAX_MIME_BYTES
                    && nonblocking(&file).is_ok()
                {
                    state.outgoing.push(Outgoing {
                        file,
                        bytes: Arc::clone(bytes),
                        offset: 0,
                        deadline: Instant::now() + TIMEOUT,
                    });
                }
            }
            ext_data_control_source_v1::Event::Cancelled => {
                source.destroy();
                state.sources.retain(|candidate| candidate != source);
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        handle: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        {
            if capabilities.contains(wl_seat::Capability::Keyboard) {
                if state.keyboard.is_none() {
                    state.keyboard = Some(seat.get_keyboard(handle, ()));
                }
            } else {
                if let Some(keyboard) = state.keyboard.take() {
                    if keyboard.version() >= 3 {
                        keyboard.release();
                    }
                }
                state.keymap = None;
            }
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for State {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_keyboard::Event::Keymap { format, fd, size } = event {
            state.keymap = None;
            let load = || -> Result<xkb::Keymap, String> {
                if format != WEnum::Value(wl_keyboard::KeymapFormat::XkbV1)
                    || size == 0
                    || size > 1024 * 1024
                {
                    return Err("Unsupported compositor keyboard keymap format or size".into());
                }
                let file = File::from(fd);
                let metadata = file.metadata().map_err(|cause| cause.to_string())?;
                if !metadata.is_file() || metadata.len() < u64::from(size) {
                    return Err("Compositor keymap is not a bounded regular file".into());
                }
                let mut bytes = vec![0; size as usize];
                file.read_exact_at(&mut bytes, 0)
                    .map_err(|cause| cause.to_string())?;
                if bytes.last() == Some(&0) {
                    bytes.pop();
                }
                let text = String::from_utf8(bytes).map_err(|cause| cause.to_string())?;
                let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
                xkb::Keymap::new_from_string(
                    &context,
                    text,
                    xkb::KEYMAP_FORMAT_TEXT_V1,
                    xkb::KEYMAP_COMPILE_NO_FLAGS,
                )
                .ok_or_else(|| "Cannot compile compositor keyboard keymap".into())
            };
            match load() {
                Ok(keymap) => {
                    state.keymap = Some(keymap);
                    state.keymap_error = None;
                }
                Err(cause) => {
                    state.keymap_error = Some(cause);
                }
            }
        }
    }
}

wayland_client::delegate_noop!(State: ignore ExtDataControlManagerV1);
wayland_client::delegate_noop!(State: ignore OrgKdeKwinFakeInput);
