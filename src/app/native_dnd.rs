use std::{
    fs::File,
    hash::{Hash, Hasher},
    io::{Read, Write},
    os::fd::OwnedFd,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc as std_mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use gio::prelude::FileExt;
use iced::{
    Point, Subscription,
    futures::{StreamExt, channel::mpsc},
    window::{Window, raw_window_handle},
};
use resvg::{tiny_skia, usvg};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    data_device_manager::{
        DataDeviceManagerState, WritePipe,
        data_device::{DataDevice, DataDeviceHandler},
        data_offer::{DataOfferHandler, DragOffer},
        data_source::{DataSourceHandler, DragSource},
    },
    delegate_compositor, delegate_data_device, delegate_output, delegate_pointer,
    delegate_registry, delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{EventLoop, LoopHandle, PostAction, channel},
        calloop_wayland_source::WaylandSource,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        pointer::{BTN_LEFT, PointerEvent, PointerEventKind, PointerHandler},
    },
    shm::{
        Shm, ShmHandler,
        slot::{Buffer, SlotPool},
    },
};
use tokio::sync::oneshot;
use wayland_backend::client::{Backend, ObjectId};
use wayland_client::{
    Connection, Proxy, QueueHandle,
    globals::registry_queue_init,
    protocol::{
        wl_data_device::WlDataDevice, wl_data_device_manager::DndAction, wl_output,
        wl_pointer::WlPointer, wl_seat::WlSeat, wl_shm, wl_surface::WlSurface,
    },
};

use crate::transfer::{Action, Adapter, AdapterCompletion, Event, Outcome, Preview};

const URI_LIST_MIME: &str = "text/uri-list";
const POLAREXP_MIME: &str = "application/x-polarexp-file-list";
const MAX_URI_LIST_BYTES: usize = 4 * 1024 * 1024;
pub(super) const ICON_SIZE: i32 = 64;
static NEXT_SOURCE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub(super) struct Source(Arc<SourceInner>);

struct SourceInner {
    id: u64,
    commands: channel::Sender<Command>,
    events: Mutex<Option<mpsc::UnboundedReceiver<Event>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        self.0.id == other.0.id
    }
}

impl Eq for Source {}

impl Hash for Source {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.id.hash(state);
    }
}

impl std::fmt::Debug for Source {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WaylandDragSource")
            .finish_non_exhaustive()
    }
}

impl Source {
    pub(super) fn attach(window: &dyn Window) -> Result<Self, String> {
        use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

        let display = match window
            .display_handle()
            .map_err(|error| format!("could not access the display handle: {error}"))?
            .as_raw()
        {
            RawDisplayHandle::Wayland(handle) => handle.display.as_ptr() as usize,
            _ => return Err("external drag-and-drop is available on Wayland".to_owned()),
        };
        let surface = match window
            .window_handle()
            .map_err(|error| format!("could not access the window handle: {error}"))?
            .as_raw()
        {
            RawWindowHandle::Wayland(handle) => handle.surface.as_ptr() as usize,
            _ => return Err("external drag-and-drop is available on Wayland".to_owned()),
        };

        let (commands, receiver) = channel::channel();
        let (events_sender, events_receiver) = mpsc::unbounded();
        let (ready_sender, ready_receiver) = std_mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("polarexp-wayland-dnd".to_owned())
            .spawn(move || {
                if let Err(error) =
                    run_worker(display, surface, receiver, events_sender, &ready_sender)
                    && ready_sender.send(Err(error.clone())).is_err()
                {
                    eprintln!("PolarExp: Wayland drag-and-drop worker stopped: {error}");
                }
            })
            .map_err(|error| format!("could not start the drag-and-drop worker: {error}"))?;

        match ready_receiver.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(())) => Ok(Self(Arc::new(SourceInner {
                id: NEXT_SOURCE_ID.fetch_add(1, Ordering::Relaxed),
                commands,
                events: Mutex::new(Some(events_receiver)),
                worker: Mutex::new(Some(worker)),
            }))),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(error) => {
                let _ = commands.send(Command::Shutdown);
                let _ = worker.join();
                Err(format!("drag-and-drop initialization timed out: {error}"))
            }
        }
    }

    fn start(
        &self,
        paths: Vec<PathBuf>,
        preview: Preview,
        copy_only: bool,
    ) -> Result<oneshot::Receiver<Result<Outcome, String>>, String> {
        let (reply, receiver) = oneshot::channel();
        self.0
            .commands
            .send(Command::Start {
                paths,
                preview,
                copy_only,
                reply,
            })
            .map_err(|_| "the Wayland drag-and-drop worker has stopped".to_owned())?;
        Ok(receiver)
    }

    pub(super) fn subscription(&self) -> Subscription<Event> {
        Subscription::run_with(self.clone(), |source| {
            source
                .0
                .events
                .lock()
                .ok()
                .and_then(|mut events| events.take())
                .map_or_else(
                    || iced::futures::stream::pending().boxed(),
                    StreamExt::boxed,
                )
        })
    }

    fn set_target(&self, id: u64, destination: Option<PathBuf>) {
        let _ = self.0.commands.send(Command::SetTarget { id, destination });
    }

    fn finish_inbound(&self, id: u64) {
        let _ = self.0.commands.send(Command::FinishInbound { id });
    }

    fn shutdown(&self) {
        let Ok(mut worker) = self.0.worker.lock() else {
            return;
        };
        let Some(worker) = worker.take() else {
            return;
        };
        let _ = self.0.commands.send(Command::Shutdown);
        let _ = worker.join();
    }
}

impl Adapter for Source {
    fn start(
        &self,
        paths: Vec<PathBuf>,
        preview: Preview,
        copy_only: bool,
    ) -> Result<AdapterCompletion, String> {
        let receiver = Source::start(self, paths, preview, copy_only)?;
        Ok(Box::pin(async move {
            receiver.await.unwrap_or_else(|_| {
                Err("the Wayland drag-and-drop worker stopped unexpectedly".to_owned())
            })
        }))
    }

    fn set_target(&self, id: u64, destination: Option<PathBuf>) {
        Source::set_target(self, id, destination);
    }

    fn finish_inbound(&self, id: u64) {
        Source::finish_inbound(self, id);
    }

    fn shutdown(&self) {
        Source::shutdown(self);
    }
}

impl Drop for SourceInner {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Ok(worker) = self.worker.get_mut()
            && let Some(worker) = worker.take()
        {
            let _ = worker.join();
        }
    }
}

enum Command {
    Start {
        paths: Vec<PathBuf>,
        preview: Preview,
        copy_only: bool,
        reply: oneshot::Sender<Result<Outcome, String>>,
    },
    SetTarget {
        id: u64,
        destination: Option<PathBuf>,
    },
    FinishInbound {
        id: u64,
    },
    Shutdown,
}

struct HeldPress {
    serial: u32,
    surface: WlSurface,
    pointer: WlPointer,
}

struct SeatObjects {
    seat: WlSeat,
    pointer: Option<WlPointer>,
    data_device: DataDevice,
}

struct ActiveDrag {
    source: DragSource,
    payload: Vec<u8>,
    reply: Option<oneshot::Sender<Result<Outcome, String>>>,
    dropped: bool,
    action: Action,
    icon_surface: WlSurface,
    _icon_buffer: Buffer,
    _icon_pool: SlotPool,
}

struct IncomingDrag {
    id: u64,
    offer: DragOffer,
    mime: String,
    private: bool,
    destination: Option<PathBuf>,
    action: Action,
    position: Point,
    data: Vec<u8>,
    reading: bool,
    dropped: bool,
}

struct Worker {
    queue_handle: QueueHandle<Worker>,
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    compositor_state: CompositorState,
    shm_state: Shm,
    data_device_manager: DataDeviceManagerState,
    origin: WlSurface,
    seats: Vec<SeatObjects>,
    held_press: Option<HeldPress>,
    active: Option<ActiveDrag>,
    incoming: Option<IncomingDrag>,
    next_offer_id: u64,
    events: mpsc::UnboundedSender<Event>,
    loop_handle: LoopHandle<'static, Worker>,
    exit: bool,
}

fn run_worker(
    display: usize,
    surface: usize,
    commands: channel::Channel<Command>,
    events: mpsc::UnboundedSender<Event>,
    ready: &std_mpsc::SyncSender<Result<(), String>>,
) -> Result<(), String> {
    // SAFETY: Iced owns both pointers for the lifetime of the window. `Source` is
    // shut down before the window is closed, and the guest backend never closes
    // the borrowed display.
    let backend = unsafe { Backend::from_foreign_display(display as *mut _) };
    let connection = Connection::from_backend(backend);
    // SAFETY: the raw handle is Iced's live wl_surface on the same connection.
    let origin_id = unsafe { ObjectId::from_ptr(WlSurface::interface(), surface as *mut _) }
        .map_err(|error| format!("could not import the Wayland surface: {error}"))?;
    let origin = WlSurface::from_id(&connection, origin_id)
        .map_err(|error| format!("could not import the Wayland surface: {error}"))?;

    let (globals, mut event_queue) = registry_queue_init(&connection)
        .map_err(|error| format!("could not read Wayland globals: {error}"))?;
    let queue_handle = event_queue.handle();
    let compositor_state = CompositorState::bind(&globals, &queue_handle)
        .map_err(|error| format!("wl_compositor is unavailable: {error}"))?;
    let shm_state = Shm::bind(&globals, &queue_handle)
        .map_err(|error| format!("wl_shm is unavailable: {error}"))?;
    let data_device_manager = DataDeviceManagerState::bind(&globals, &queue_handle)
        .map_err(|error| format!("wl_data_device_manager is unavailable: {error}"))?;
    let mut event_loop: EventLoop<'static, Worker> =
        EventLoop::try_new().map_err(|error| format!("could not create an event loop: {error}"))?;
    let handle = event_loop.handle();
    let mut state = Worker {
        queue_handle: queue_handle.clone(),
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &queue_handle),
        output_state: OutputState::new(&globals, &queue_handle),
        compositor_state,
        shm_state,
        data_device_manager,
        origin,
        seats: Vec::new(),
        held_press: None,
        active: None,
        incoming: None,
        next_offer_id: 1,
        events,
        loop_handle: handle.clone(),
        exit: false,
    };

    event_queue
        .roundtrip(&mut state)
        .map_err(|error| format!("could not initialize the Wayland seat: {error}"))?;
    if state.seats.iter().all(|seat| seat.pointer.is_none()) {
        return Err("the Wayland seat has no pointer capability".to_owned());
    }

    WaylandSource::new(connection, event_queue)
        .insert(handle.clone())
        .map_err(|error| format!("could not register the Wayland event queue: {error}"))?;
    handle
        .insert_source(commands, |event, _, state| match event {
            channel::Event::Msg(Command::Start {
                paths,
                preview,
                copy_only,
                reply,
            }) => state.start_drag(paths, preview, copy_only, reply),
            channel::Event::Msg(Command::SetTarget { id, destination }) => {
                state.set_target(id, destination)
            }
            channel::Event::Msg(Command::FinishInbound { id }) => state.finish_inbound(id),
            channel::Event::Msg(Command::Shutdown) | channel::Event::Closed => {
                state.finish_active(Outcome::Cancelled);
                state.cancel_incoming();
                state.exit = true;
            }
        })
        .map_err(|error| format!("could not register the command channel: {error}"))?;
    let _ = ready.send(Ok(()));

    while !state.exit {
        event_loop
            .dispatch(Duration::from_millis(250), &mut state)
            .map_err(|error| format!("Wayland drag-and-drop failed: {error}"))?;
    }
    Ok(())
}

impl Worker {
    fn start_drag(
        &mut self,
        paths: Vec<PathBuf>,
        preview: Preview,
        copy_only: bool,
        reply: oneshot::Sender<Result<Outcome, String>>,
    ) {
        let result = self.try_start_drag(paths, preview, copy_only, reply);
        if let Err((error, reply)) = result {
            let _ = reply.send(Err(error));
        }
    }

    fn try_start_drag(
        &mut self,
        paths: Vec<PathBuf>,
        preview: Preview,
        copy_only: bool,
        reply: oneshot::Sender<Result<Outcome, String>>,
    ) -> Result<(), (String, oneshot::Sender<Result<Outcome, String>>)> {
        if self.active.is_some() {
            return Err(("another external drag is already active".to_owned(), reply));
        }
        if paths.is_empty() {
            return Err(("there is nothing to drag".to_owned(), reply));
        }
        let Some(held) = self.held_press.take() else {
            return Err((
                "the pointer grab ended before the drag started".to_owned(),
                reply,
            ));
        };
        if held.surface != self.origin {
            return Err((
                "the pointer grab did not originate in PolarExp".to_owned(),
                reply,
            ));
        }
        let Some(data_device_index) = self
            .seats
            .iter()
            .position(|seat| seat.pointer.as_ref() == Some(&held.pointer))
        else {
            return Err(("the pointer seat has no data device".to_owned(), reply));
        };
        let payload = match paths_to_uri_list(&paths) {
            Ok(payload) => payload.into_bytes(),
            Err(error) => return Err((error, reply)),
        };
        let (icon_surface, icon_buffer, icon_pool) = match self.create_icon(preview) {
            Ok(icon) => icon,
            Err(error) => return Err((error, reply)),
        };
        let source = self.data_device_manager.create_drag_and_drop_source(
            &self.queue_handle,
            [URI_LIST_MIME, POLAREXP_MIME],
            if copy_only {
                DndAction::Copy
            } else {
                DndAction::Copy | DndAction::Move
            },
        );
        source.start_drag(
            &self.seats[data_device_index].data_device,
            &self.origin,
            Some(&icon_surface),
            held.serial,
        );
        icon_surface.commit();
        self.active = Some(ActiveDrag {
            source,
            payload,
            reply: Some(reply),
            dropped: false,
            action: Action::Copy,
            icon_surface,
            _icon_buffer: icon_buffer,
            _icon_pool: icon_pool,
        });
        Ok(())
    }

    fn create_icon(&mut self, preview: Preview) -> Result<(WlSurface, Buffer, SlotPool), String> {
        let mut pixels = render_icon(preview)?;
        let mut pool = SlotPool::new((ICON_SIZE * ICON_SIZE * 4) as usize, &self.shm_state)
            .map_err(|error| format!("could not create the drag icon pool: {error}"))?;
        let (buffer, canvas) = pool
            .create_buffer(
                ICON_SIZE,
                ICON_SIZE,
                ICON_SIZE * 4,
                wl_shm::Format::Argb8888,
            )
            .map_err(|error| format!("could not create the drag icon buffer: {error}"))?;
        for (target, source) in canvas.chunks_exact_mut(4).zip(pixels.chunks_exact_mut(4)) {
            target.copy_from_slice(&[source[2], source[1], source[0], source[3]]);
        }
        let surface = self.compositor_state.create_surface(&self.queue_handle);
        buffer
            .attach_to(&surface)
            .map_err(|error| format!("could not attach the drag icon: {error}"))?;
        if surface.version() >= 5 {
            surface.offset(14, 16);
        }
        surface.damage_buffer(0, 0, ICON_SIZE, ICON_SIZE);
        Ok((surface, buffer, pool))
    }

    fn finish_active(&mut self, outcome: Outcome) {
        if let Some(mut active) = self.active.take() {
            active.icon_surface.destroy();
            if let Some(reply) = active.reply.take() {
                let _ = reply.send(Ok(outcome));
            }
        }
    }

    fn set_target(&mut self, id: u64, destination: Option<PathBuf>) {
        let Some(incoming) = self.incoming.as_mut().filter(|incoming| incoming.id == id) else {
            return;
        };
        incoming.destination = destination;
        if incoming.destination.is_some() {
            let accepted = if incoming.private {
                DndAction::Copy | DndAction::Move
            } else {
                DndAction::Copy
            };
            incoming
                .offer
                .accept_mime_type(incoming.offer.serial, Some(incoming.mime.clone()));
            incoming
                .offer
                .set_actions(accepted, dnd_action(incoming.action));
        } else {
            incoming.offer.accept_mime_type(incoming.offer.serial, None);
            incoming
                .offer
                .set_actions(DndAction::empty(), DndAction::empty());
        }
    }

    fn finish_inbound(&mut self, id: u64) {
        let Some(incoming) = self.incoming.take() else {
            return;
        };
        if incoming.id != id {
            self.incoming = Some(incoming);
            return;
        }
        incoming.offer.finish();
        incoming.offer.destroy();
    }

    fn cancel_incoming(&mut self) {
        if let Some(incoming) = self.incoming.take() {
            incoming.offer.destroy();
        }
    }

    fn fail_incoming(&mut self, message: impl Into<String>) {
        let _ = self.events.unbounded_send(Event::Error(message.into()));
        self.cancel_incoming();
    }

    fn complete_incoming_read(&mut self, id: u64) {
        let Some(incoming) = self.incoming.as_ref().filter(|incoming| incoming.id == id) else {
            return;
        };
        match parse_uri_list(&incoming.data) {
            Ok(paths) => {
                let _ = self.events.unbounded_send(Event::Drop {
                    id,
                    paths,
                    destination: incoming.destination.clone().expect("validated drop target"),
                    action: incoming.action,
                });
            }
            Err(error) => self.fail_incoming(error),
        }
    }
}

fn render_icon(preview: Preview) -> Result<Vec<u8>, String> {
    let mut pixmap = tiny_skia::Pixmap::new(ICON_SIZE as u32, ICON_SIZE as u32)
        .ok_or_else(|| "could not allocate the drag icon".to_owned())?;
    let svg = preview_svg(preview)?;
    let mut options = usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_data(&svg, &options)
        .map_err(|error| format!("could not render the drag icon: {error}"))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    Ok(pixmap.take())
}

pub(super) fn preview_svg(preview: Preview) -> Result<Vec<u8>, String> {
    let icon = std::str::from_utf8(preview.icon)
        .map_err(|error| format!("invalid drag preview icon: {error}"))?;
    let icon_body = icon
        .split_once('>')
        .and_then(|(_, body)| body.rsplit_once("</svg>").map(|(body, _)| body))
        .ok_or_else(|| "invalid drag preview icon".to_owned())?
        .replace("#000", &rgb(preview.icon_color));
    let badge = match (preview.count, preview.copy) {
        (1, false) => String::new(),
        (1, true) => badge_svg("+", 20, preview),
        (count, false) => badge_svg(&count.to_string(), badge_width(count, false), preview),
        (count, true) => badge_svg(&format!("{count} +"), badge_width(count, true), preview),
    };
    Ok(format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64">
<defs><filter id="s" x="-30%" y="-30%" width="170%" height="180%"><feDropShadow dx="0" dy="4" stdDeviation="4" flood-color="#000" flood-opacity=".34"/></filter></defs>
<rect x="2.5" y="2.5" width="51" height="51" rx="9" fill="{}" fill-opacity="{}" stroke="{}" stroke-opacity=".75" filter="url(#s)"/>
<g transform="translate(11 11) scale(1.4166667)">{icon_body}</g>{badge}</svg>"##,
        rgb(preview.background),
        alpha(preview.background),
        rgb(preview.accent),
    )
    .into_bytes())
}

fn badge_width(count: usize, copy: bool) -> usize {
    let digits = count.to_string().len();
    (if copy {
        22 + digits * 7
    } else {
        14 + digits * 7
    })
    .clamp(20, 60)
}

fn badge_svg(label: &str, width: usize, preview: Preview) -> String {
    let x = 62_usize.saturating_sub(width);
    let center = x as f32 + width as f32 / 2.0;
    format!(
        r#"<rect x="{x}" y="40" width="{width}" height="20" rx="10" fill="{}"/><text x="{center}" y="50.5" fill="{}" font-family="sans-serif" font-size="11" font-weight="600" text-anchor="middle" dominant-baseline="middle">{label}</text>"#,
        rgb(preview.accent),
        rgb(preview.badge_text),
    )
}

fn rgb(color: [u8; 4]) -> String {
    format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2])
}

fn alpha(color: [u8; 4]) -> f32 {
    f32::from(color[3]) / 255.0
}

pub(super) fn paths_to_uri_list(paths: &[PathBuf]) -> Result<String, String> {
    let mut payload = String::new();
    for path in paths {
        if !path.is_absolute() {
            return Err(format!("cannot drag a relative path: {}", path.display()));
        }
        payload.push_str(gio::File::for_path(path).uri().as_str());
        payload.push_str("\r\n");
    }
    Ok(payload)
}

pub(super) fn parse_uri_list(payload: &[u8]) -> Result<Vec<PathBuf>, String> {
    if payload.len() > MAX_URI_LIST_BYTES {
        return Err("the dropped URI list is larger than 4 MiB".to_owned());
    }
    let text = std::str::from_utf8(payload)
        .map_err(|error| format!("the dropped URI list is not UTF-8: {error}"))?;
    let mut paths = Vec::new();
    for line in text.lines() {
        let uri = line.trim_end_matches('\r').trim();
        if uri.is_empty() || uri.starts_with('#') {
            continue;
        }
        let file = gio::File::for_uri(uri);
        if file.uri_scheme().as_deref() != Some("file") {
            return Err(format!("unsupported dropped URI: {uri}"));
        }
        let path = file
            .path()
            .filter(|path| path.is_absolute())
            .ok_or_else(|| format!("unsupported dropped URI: {uri}"))?;
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    if paths.is_empty() {
        return Err("the drop did not contain any local file paths".to_owned());
    }
    Ok(paths)
}

fn preferred_action(private: bool, source_actions: DndAction) -> Action {
    if private && (source_actions.is_empty() || source_actions.contains(DndAction::Move)) {
        Action::Move
    } else {
        Action::Copy
    }
}

fn dnd_action(action: Action) -> DndAction {
    match action {
        Action::Copy => DndAction::Copy,
        Action::Move => DndAction::Move,
    }
}

fn action_from_dnd(action: DndAction, fallback: Action) -> Action {
    if action.contains(DndAction::Move) {
        Action::Move
    } else if action.contains(DndAction::Copy) {
        Action::Copy
    } else {
        fallback
    }
}

impl CompositorHandler for Worker {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlSurface, _: u32) {}

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for Worker {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for Worker {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm_state
    }
}

impl SeatHandler for Worker {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        queue: &QueueHandle<Self>,
        seat: WlSeat,
        capability: Capability,
    ) {
        if capability != Capability::Pointer {
            return;
        }
        let index = self.seats.iter().position(|objects| objects.seat == seat);
        let index = index.unwrap_or_else(|| {
            let data_device = self.data_device_manager.get_data_device(queue, &seat);
            self.seats.push(SeatObjects {
                seat: seat.clone(),
                pointer: None,
                data_device,
            });
            self.seats.len() - 1
        });
        if self.seats[index].pointer.is_none()
            && let Ok(pointer) = self.seat_state.get_pointer(queue, &seat)
        {
            self.seats[index].pointer = Some(pointer);
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        seat: WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer
            && let Some(objects) = self.seats.iter_mut().find(|objects| objects.seat == seat)
            && let Some(pointer) = objects.pointer.take()
        {
            if self
                .held_press
                .as_ref()
                .is_some_and(|held| held.pointer == pointer)
            {
                self.held_press = None;
            }
            pointer.release();
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: WlSeat) {
        self.seats.retain(|objects| objects.seat != seat);
    }
}

impl PointerHandler for Worker {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        pointer: &WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            match event.kind {
                PointerEventKind::Press { button, serial, .. } if button == BTN_LEFT => {
                    self.held_press = Some(HeldPress {
                        serial,
                        surface: event.surface.clone(),
                        pointer: pointer.clone(),
                    });
                }
                PointerEventKind::Release { button, .. }
                    if button == BTN_LEFT
                        && self
                            .held_press
                            .as_ref()
                            .is_some_and(|held| &held.pointer == pointer) =>
                {
                    self.held_press = None;
                }
                _ => {}
            }
        }
    }
}

impl DataDeviceHandler for Worker {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        data_device: &WlDataDevice,
        x: f64,
        y: f64,
        surface: &WlSurface,
    ) {
        if surface != &self.origin {
            return;
        }
        self.cancel_incoming();
        if let Some(device) = self
            .seats
            .iter()
            .find(|seat| seat.data_device.inner() == data_device)
            && let Some(offer) = device.data_device.data().drag_offer()
        {
            let mime = offer.with_mime_types(|mimes| {
                mimes
                    .iter()
                    .find(|mime| mime.as_str() == POLAREXP_MIME)
                    .or_else(|| mimes.iter().find(|mime| mime.as_str() == URI_LIST_MIME))
                    .cloned()
            });
            let Some(mime) = mime else {
                offer.accept_mime_type(offer.serial, None);
                offer.set_actions(DndAction::empty(), DndAction::empty());
                return;
            };
            let private = mime == POLAREXP_MIME;
            let action = preferred_action(private, offer.source_actions);
            let id = self.next_offer_id;
            self.next_offer_id += 1;
            let position = Point::new(x as f32, y as f32);
            self.incoming = Some(IncomingDrag {
                id,
                offer: offer.clone(),
                mime,
                private,
                destination: None,
                action,
                position,
                data: Vec::new(),
                reading: false,
                dropped: false,
            });
            let _ = self.events.unbounded_send(Event::Hover {
                id,
                position,
                action,
            });
        }
    }

    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {
        let Some(incoming) = self.incoming.as_ref() else {
            return;
        };
        let id = incoming.id;
        let dropped = incoming.dropped;
        let _ = self.events.unbounded_send(Event::Leave { id });
        if !dropped {
            self.cancel_incoming();
        }
    }

    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice, x: f64, y: f64) {
        let Some(incoming) = self.incoming.as_mut() else {
            return;
        };
        incoming.position = Point::new(x as f32, y as f32);
        let _ = self.events.unbounded_send(Event::Hover {
            id: incoming.id,
            position: incoming.position,
            action: incoming.action,
        });
    }
    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}
    fn drop_performed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {
        let Some(incoming) = self.incoming.as_mut() else {
            return;
        };
        if incoming.destination.is_none() || incoming.reading {
            self.cancel_incoming();
            return;
        }
        incoming.dropped = true;
        incoming.reading = true;
        let id = incoming.id;
        let pipe = match incoming.offer.receive(incoming.mime.clone()) {
            Ok(pipe) => pipe,
            Err(error) => {
                self.fail_incoming(format!("could not receive dropped files: {error}"));
                return;
            }
        };
        if let Err(error) = self.loop_handle.insert_source(pipe, move |_, file, state| {
            let mut buffer = [0_u8; 8192];
            // SAFETY: calloop owns the file for the callback lifetime and it is not closed here.
            let result = unsafe { file.get_mut() }.read(&mut buffer);
            match result {
                Ok(0) => {
                    state.complete_incoming_read(id);
                    PostAction::Remove
                }
                Ok(count) => {
                    let Some(incoming) =
                        state.incoming.as_mut().filter(|incoming| incoming.id == id)
                    else {
                        return PostAction::Remove;
                    };
                    if incoming.data.len().saturating_add(count) > MAX_URI_LIST_BYTES {
                        state.fail_incoming("the dropped URI list is larger than 4 MiB");
                        PostAction::Remove
                    } else {
                        incoming.data.extend_from_slice(&buffer[..count]);
                        PostAction::Continue
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    PostAction::Continue
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
                    PostAction::Continue
                }
                Err(error) => {
                    state.fail_incoming(format!("could not read dropped files: {error}"));
                    PostAction::Remove
                }
            }
        }) {
            self.fail_incoming(format!("could not monitor dropped files: {error}"));
        }
    }
}

impl DataOfferHandler for Worker {
    fn source_actions(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        offer: &mut DragOffer,
        actions: DndAction,
    ) {
        let Some(incoming) = self
            .incoming
            .as_mut()
            .filter(|incoming| incoming.offer == *offer)
        else {
            return;
        };
        incoming.action = preferred_action(incoming.private, actions);
        let _ = self.events.unbounded_send(Event::Hover {
            id: incoming.id,
            position: incoming.position,
            action: incoming.action,
        });
    }

    fn selected_action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        offer: &mut DragOffer,
        action: DndAction,
    ) {
        let Some(incoming) = self
            .incoming
            .as_mut()
            .filter(|incoming| incoming.offer == *offer)
        else {
            return;
        };
        incoming.action = action_from_dnd(action, incoming.action);
        let _ = self.events.unbounded_send(Event::Hover {
            id: incoming.id,
            position: incoming.position,
            action: incoming.action,
        });
    }
}

impl DataSourceHandler for Worker {
    fn accept_mime(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wayland_client::protocol::wl_data_source::WlDataSource,
        _: Option<String>,
    ) {
    }

    fn send_request(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &wayland_client::protocol::wl_data_source::WlDataSource,
        mime: String,
        pipe: WritePipe,
    ) {
        if mime != URI_LIST_MIME && mime != POLAREXP_MIME {
            return;
        }
        if let Some(active) = self
            .active
            .as_ref()
            .filter(|drag| drag.source.inner() == source)
        {
            let mut file = File::from(OwnedFd::from(pipe));
            let _ = file.write_all(&active.payload);
        }
    }

    fn cancelled(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &wayland_client::protocol::wl_data_source::WlDataSource,
    ) {
        if self
            .active
            .as_ref()
            .is_some_and(|drag| drag.source.inner() == source)
        {
            self.finish_active(Outcome::Cancelled);
        }
    }

    fn dnd_dropped(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &wayland_client::protocol::wl_data_source::WlDataSource,
    ) {
        if let Some(active) = self
            .active
            .as_mut()
            .filter(|drag| drag.source.inner() == source)
        {
            active.dropped = true;
        }
    }

    fn dnd_finished(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &wayland_client::protocol::wl_data_source::WlDataSource,
    ) {
        if self
            .active
            .as_ref()
            .is_some_and(|drag| drag.source.inner() == source)
        {
            let outcome = if let Some(drag) = self.active.as_ref().filter(|drag| drag.dropped) {
                Outcome::Dropped(drag.action)
            } else {
                Outcome::Cancelled
            };
            self.finish_active(outcome);
        }
    }

    fn action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &wayland_client::protocol::wl_data_source::WlDataSource,
        action: DndAction,
    ) {
        if let Some(active) = self
            .active
            .as_mut()
            .filter(|drag| drag.source.inner() == source)
        {
            active.action = action_from_dnd(action, active.action);
        }
    }
}

impl ProvidesRegistryState for Worker {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(Worker);
delegate_output!(Worker);
delegate_shm!(Worker);
delegate_seat!(Worker);
delegate_pointer!(Worker);
delegate_data_device!(Worker);
delegate_registry!(Worker);

#[cfg(test)]
mod tests {
    use super::{
        Action, ICON_SIZE, MAX_URI_LIST_BYTES, Preview, parse_uri_list, paths_to_uri_list,
        preferred_action, preview_svg, render_icon,
    };
    use smithay_client_toolkit::reexports::client::protocol::wl_data_device_manager::DndAction;
    use std::{os::unix::fs::symlink, path::PathBuf};

    #[test]
    fn uri_list_uses_crlf_and_escapes_paths() {
        let payload = paths_to_uri_list(&[
            PathBuf::from("/tmp/a file.txt"),
            PathBuf::from("/tmp/second.txt"),
        ])
        .expect("URI list");
        assert_eq!(
            payload,
            "file:///tmp/a%20file.txt\r\nfile:///tmp/second.txt\r\n"
        );
    }

    #[test]
    fn uri_list_preserves_symlink_names() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let target = directory.path().join("target");
        std::fs::write(&target, "data").expect("target");
        let link = directory.path().join("visible-link");
        symlink(&target, &link).expect("symlink");

        let payload = paths_to_uri_list(std::slice::from_ref(&link)).expect("URI list");
        assert!(payload.ends_with("/visible-link\r\n"));
        assert!(!payload.ends_with("/target\r\n"));
    }

    #[test]
    fn uri_list_rejects_relative_paths() {
        assert!(paths_to_uri_list(&[PathBuf::from("relative")]).is_err());
    }

    #[test]
    fn parses_uri_lists_with_comments_crlf_and_escaped_names() {
        let paths = parse_uri_list(
            b"# generated by source\r\nfile:///tmp/a%20file.txt\r\n\nfile:///tmp/b.txt\n",
        )
        .expect("parsed URI list");
        assert_eq!(
            paths,
            [
                PathBuf::from("/tmp/a file.txt"),
                PathBuf::from("/tmp/b.txt")
            ]
        );
    }

    #[test]
    fn parser_rejects_non_file_empty_and_oversize_payloads() {
        assert!(parse_uri_list(b"https://example.com/file").is_err());
        assert!(parse_uri_list(b"# only a comment\n").is_err());
        assert!(parse_uri_list(&vec![b'x'; MAX_URI_LIST_BYTES + 1]).is_err());
    }

    #[test]
    fn private_offers_prefer_move_but_copy_only_is_respected() {
        assert_eq!(
            preferred_action(true, DndAction::Copy | DndAction::Move),
            Action::Move
        );
        assert_eq!(preferred_action(true, DndAction::Copy), Action::Copy);
        assert_eq!(preferred_action(false, DndAction::Move), Action::Copy);
    }

    #[test]
    fn shared_drag_preview_renders_the_icon_count_and_copy_marker() {
        let preview = Preview {
            icon: include_bytes!("../ui/icons/file-code.svg"),
            count: 3,
            copy: true,
            background: [28, 28, 28, 235],
            icon_color: [210, 210, 210, 255],
            accent: [40, 120, 220, 255],
            badge_text: [255, 255, 255, 255],
        };
        let svg = String::from_utf8(preview_svg(preview).expect("preview SVG")).expect("UTF-8");
        assert!(svg.contains("3 +"));
        assert!(svg.contains("#d2d2d2"));
        assert_eq!(
            render_icon(preview).expect("rendered preview").len(),
            (ICON_SIZE * ICON_SIZE * 4) as usize
        );
    }
}
