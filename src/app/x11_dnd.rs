use std::{
    hash::{Hash, Hasher},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use iced::window::{Window as IcedWindow, raw_window_handle};
use tokio::sync::oneshot;
use x11rb::{
    COPY_DEPTH_FROM_PARENT, CURRENT_TIME,
    connection::Connection,
    protocol::{
        Event,
        xproto::{
            Atom, AtomEnum, ClientMessageData, ClientMessageEvent, ConfigureWindowAux,
            ConnectionExt, CreateGCAux, CreateWindowAux, EventMask, ImageFormat, KeyButMask,
            PropMode, SELECTION_NOTIFY_EVENT, SelectionNotifyEvent, SelectionRequestEvent,
            StackMode, Window, WindowClass,
        },
    },
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

use crate::{
    transfer::{Action, Adapter, AdapterCompletion, Outcome, Preview},
    transfer_formats,
};

use super::native_dnd;

const POLL_INTERVAL: Duration = Duration::from_millis(10);
const FINISH_TIMEOUT: Duration = Duration::from_secs(10);
static NEXT_SOURCE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub(super) struct Source(Arc<SourceInner>);

struct SourceInner {
    id: u64,
    commands: mpsc::Sender<Command>,
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
            .debug_struct("X11DragSource")
            .finish_non_exhaustive()
    }
}

impl Source {
    pub(super) fn attach(window: &dyn IcedWindow) -> Result<Self, String> {
        use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

        match window
            .display_handle()
            .map_err(|error| format!("could not access the X11 display handle: {error}"))?
            .as_raw()
        {
            RawDisplayHandle::Xlib(_) | RawDisplayHandle::Xcb(_) => {}
            _ => return Err("the X11 drag adapter requires an X11 window".to_owned()),
        }
        let origin = match window
            .window_handle()
            .map_err(|error| format!("could not access the X11 window handle: {error}"))?
            .as_raw()
        {
            RawWindowHandle::Xlib(handle) => u32::try_from(handle.window)
                .map_err(|_| "the Xlib window ID does not fit X11".to_owned())?,
            RawWindowHandle::Xcb(handle) => handle.window.get(),
            _ => return Err("the X11 drag adapter requires an X11 window".to_owned()),
        };
        Self::attach_window(origin)
    }

    fn attach_window(origin: Window) -> Result<Self, String> {
        let (commands, receiver) = mpsc::channel();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("waddle-x11-dnd".to_owned())
            .spawn(move || {
                let result = Worker::connect(origin).and_then(|mut worker| {
                    let _ = ready_sender.send(Ok(()));
                    worker.run(receiver)
                });
                if let Err(error) = result {
                    let _ = ready_sender.send(Err(error));
                }
            })
            .map_err(|error| format!("could not start the X11 drag worker: {error}"))?;
        match ready_receiver.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(())) => Ok(Self(Arc::new(SourceInner {
                id: NEXT_SOURCE_ID.fetch_add(1, Ordering::Relaxed),
                commands,
                worker: Mutex::new(Some(worker)),
            }))),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(error) => {
                let _ = commands.send(Command::Shutdown);
                let _ = worker.join();
                Err(format!("X11 drag initialization timed out: {error}"))
            }
        }
    }

    fn start_inner(
        &self,
        paths: Vec<PathBuf>,
        preview: Preview,
        copy_only: bool,
        synthetic_hold: bool,
    ) -> Result<AdapterCompletion, String> {
        let (reply, receiver) = oneshot::channel();
        self.0
            .commands
            .send(Command::Start {
                paths,
                preview,
                copy_only,
                synthetic_hold,
                reply,
            })
            .map_err(|_| "the X11 drag worker has stopped".to_owned())?;
        Ok(Box::pin(async move {
            receiver
                .await
                .unwrap_or_else(|_| Err("the X11 drag worker stopped unexpectedly".to_owned()))
        }))
    }

    #[cfg(test)]
    fn release_synthetic_hold(&self) {
        let _ = self.0.commands.send(Command::ReleaseSyntheticHold);
    }

    pub(super) fn incoming_action(&self) -> Action {
        let (reply, receiver) = mpsc::sync_channel(1);
        if self
            .0
            .commands
            .send(Command::IncomingAction { reply })
            .is_err()
        {
            return Action::Copy;
        }
        receiver
            .recv_timeout(Duration::from_millis(250))
            .unwrap_or(Action::Copy)
    }
}

impl Adapter for Source {
    fn start(
        &self,
        paths: Vec<PathBuf>,
        preview: Preview,
        copy_only: bool,
    ) -> Result<AdapterCompletion, String> {
        self.start_inner(paths, preview, copy_only, false)
    }

    fn set_target(&self, _: u64, _: Option<PathBuf>) {}

    fn finish_inbound(&self, _: u64) {}

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
        synthetic_hold: bool,
        reply: oneshot::Sender<Result<Outcome, String>>,
    },
    #[cfg(test)]
    ReleaseSyntheticHold,
    IncomingAction {
        reply: mpsc::SyncSender<Action>,
    },
    Shutdown,
}

struct Atoms {
    xdnd_aware: Atom,
    xdnd_enter: Atom,
    xdnd_position: Atom,
    xdnd_status: Atom,
    xdnd_leave: Atom,
    xdnd_drop: Atom,
    xdnd_finished: Atom,
    xdnd_selection: Atom,
    xdnd_type_list: Atom,
    xdnd_action_list: Atom,
    action_copy: Atom,
    action_move: Atom,
    targets: Atom,
    uri_list: Atom,
    private_type: Atom,
    waddle_action: Atom,
}

struct ActiveDrag {
    payload: Vec<u8>,
    desired_action: Action,
    target: Option<Window>,
    accepted: bool,
    accepted_action: Action,
    last_position: Option<(Window, i16, i16)>,
    waiting_for_finish: bool,
    synthetic_hold: bool,
    dropped_at: Option<Instant>,
    reply: Option<oneshot::Sender<Result<Outcome, String>>>,
}

struct Worker {
    connection: RustConnection,
    root: Window,
    root_depth: u8,
    origin: Window,
    source: Window,
    icon: Window,
    atoms: Atoms,
    active: Option<ActiveDrag>,
    exit: bool,
}

impl Worker {
    fn connect(origin: Window) -> Result<Self, String> {
        let (connection, screen_index) = RustConnection::connect(None)
            .map_err(|error| format!("could not connect to X11 for drag-and-drop: {error}"))?;
        let screen = connection
            .setup()
            .roots
            .get(screen_index)
            .ok_or_else(|| "the X11 drag screen is unavailable".to_owned())?;
        let root = screen.root;
        let root_depth = screen.root_depth;
        let root_visual = screen.root_visual;
        let source = connection
            .generate_id()
            .map_err(|error| error.to_string())?;
        connection
            .create_window(
                COPY_DEPTH_FROM_PARENT,
                source,
                root,
                -1,
                -1,
                1,
                1,
                0,
                WindowClass::INPUT_OUTPUT,
                root_visual,
                &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
            )
            .map_err(|error| format!("could not create the X11 drag source window: {error}"))?;
        let icon = connection
            .generate_id()
            .map_err(|error| error.to_string())?;
        connection
            .create_window(
                root_depth,
                icon,
                root,
                -100,
                -100,
                native_dnd::ICON_SIZE as u16,
                native_dnd::ICON_SIZE as u16,
                0,
                WindowClass::INPUT_OUTPUT,
                root_visual,
                &CreateWindowAux::new().override_redirect(1),
            )
            .map_err(|error| format!("could not create the X11 drag preview: {error}"))?;
        let atoms = Atoms::intern(&connection)?;
        connection
            .change_property32(
                PropMode::REPLACE,
                source,
                atoms.xdnd_type_list,
                AtomEnum::ATOM,
                &[atoms.uri_list, atoms.private_type],
            )
            .map_err(|error| format!("could not publish X11 drag types: {error}"))?;
        connection
            .flush()
            .map_err(|error| format!("could not initialize X11 drag-and-drop: {error}"))?;
        Ok(Self {
            connection,
            root,
            root_depth,
            origin,
            source,
            icon,
            atoms,
            active: None,
            exit: false,
        })
    }

    fn run(&mut self, commands: mpsc::Receiver<Command>) -> Result<(), String> {
        while !self.exit {
            while let Ok(command) = commands.try_recv() {
                match command {
                    Command::Start {
                        paths,
                        preview,
                        copy_only,
                        synthetic_hold,
                        reply,
                    } => self.start(paths, preview, copy_only, synthetic_hold, reply),
                    #[cfg(test)]
                    Command::ReleaseSyntheticHold => {
                        if let Some(active) = self.active.as_mut() {
                            active.synthetic_hold = false;
                        }
                    }
                    Command::IncomingAction { reply } => {
                        let _ = reply.send(self.read_incoming_action());
                    }
                    Command::Shutdown => {
                        self.finish(Ok(Outcome::Cancelled));
                        self.exit = true;
                    }
                }
            }
            while let Some(event) = self
                .connection
                .poll_for_event()
                .map_err(|error| format!("X11 drag event failed: {error}"))?
            {
                self.handle_event(event)?;
            }
            self.poll_pointer()?;
            thread::sleep(POLL_INTERVAL);
        }
        Ok(())
    }

    fn start(
        &mut self,
        paths: Vec<PathBuf>,
        preview: Preview,
        copy_only: bool,
        synthetic_hold: bool,
        reply: oneshot::Sender<Result<Outcome, String>>,
    ) {
        if self.active.is_some() {
            let _ = reply.send(Err("another X11 drag is already active".to_owned()));
            return;
        }
        let payload = match transfer_formats::encode_uri_list(&paths) {
            Ok(payload) => payload,
            Err(error) => {
                let _ = reply.send(Err(error));
                return;
            }
        };
        let desired_action = if copy_only {
            Action::Copy
        } else {
            Action::Move
        };
        if let Err(error) = self.prepare_icon(preview).and_then(|()| {
            let actions = [self.atoms.action_copy, self.atoms.action_move];
            let actions = if copy_only {
                &actions[..1]
            } else {
                &actions[..]
            };
            self.connection
                .change_property32(
                    PropMode::REPLACE,
                    self.source,
                    self.atoms.xdnd_action_list,
                    AtomEnum::ATOM,
                    actions,
                )
                .map_err(|error| format!("could not publish X11 drag actions: {error}"))?;
            self.connection
                .set_selection_owner(self.source, self.atoms.xdnd_selection, CURRENT_TIME)
                .map_err(|error| format!("could not own the X11 drag selection: {error}"))?;
            self.connection
                .change_property32(
                    PropMode::REPLACE,
                    self.root,
                    self.atoms.waddle_action,
                    AtomEnum::CARDINAL,
                    &[self.source, self.action_atom(desired_action)],
                )
                .map_err(|error| format!("could not publish the Waddle X11 action: {error}"))?;
            self.connection
                .map_window(self.icon)
                .map_err(|error| format!("could not show the X11 drag preview: {error}"))?;
            self.connection
                .flush()
                .map_err(|error| format!("could not start X11 drag-and-drop: {error}"))
        }) {
            let _ = reply.send(Err(error));
            return;
        }
        self.active = Some(ActiveDrag {
            payload,
            desired_action,
            target: None,
            accepted: false,
            accepted_action: desired_action,
            last_position: None,
            waiting_for_finish: false,
            synthetic_hold,
            dropped_at: None,
            reply: Some(reply),
        });
    }

    fn prepare_icon(&self, preview: Preview) -> Result<(), String> {
        let rgba = native_dnd::render_icon(preview)?;
        let mut pixels = Vec::with_capacity(rgba.len());
        for pixel in rgba.chunks_exact(4) {
            pixels.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 0]);
        }
        let gc = self
            .connection
            .generate_id()
            .map_err(|error| error.to_string())?;
        self.connection
            .create_gc(gc, self.icon, &CreateGCAux::new())
            .map_err(|error| format!("could not create the X11 drag preview context: {error}"))?;
        let result = self
            .connection
            .put_image(
                ImageFormat::Z_PIXMAP,
                self.icon,
                gc,
                native_dnd::ICON_SIZE as u16,
                native_dnd::ICON_SIZE as u16,
                0,
                0,
                0,
                self.root_depth,
                &pixels,
            )
            .map_err(|error| format!("could not draw the X11 drag preview: {error}"));
        let _ = self.connection.free_gc(gc);
        result.map(|_| ())
    }

    fn poll_pointer(&mut self) -> Result<(), String> {
        let Some(active) = self.active.as_ref() else {
            return Ok(());
        };
        if active.waiting_for_finish {
            if active
                .dropped_at
                .is_some_and(|started| started.elapsed() >= FINISH_TIMEOUT)
            {
                self.finish(Err(
                    "the X11 drop target did not finish the transfer".to_owned()
                ));
            }
            return Ok(());
        }
        let pointer = self
            .connection
            .query_pointer(self.root)
            .map_err(|error| format!("could not query the X11 pointer: {error}"))?
            .reply()
            .map_err(|error| format!("could not query the X11 pointer: {error}"))?;
        self.connection
            .configure_window(
                self.icon,
                &ConfigureWindowAux::new()
                    .x(i32::from(pointer.root_x) + 14)
                    .y(i32::from(pointer.root_y) + 16)
                    .stack_mode(StackMode::ABOVE),
            )
            .map_err(|error| format!("could not move the X11 drag preview: {error}"))?;
        let target = self.target_at_pointer()?;
        self.update_target(target, pointer.root_x, pointer.root_y)?;
        let held = pointer.mask.contains(KeyButMask::BUTTON1)
            || self
                .active
                .as_ref()
                .is_some_and(|active| active.synthetic_hold);
        if !held {
            let (target, accepted, action) = self
                .active
                .as_ref()
                .map_or((None, false, Action::Copy), |active| {
                    (active.target, active.accepted, active.accepted_action)
                });
            if let Some(target) = target.filter(|_| accepted) {
                self.send_client(
                    target,
                    self.atoms.xdnd_drop,
                    [self.source, 0, CURRENT_TIME, 0, 0],
                )?;
                if let Some(active) = self.active.as_mut() {
                    active.waiting_for_finish = true;
                    active.dropped_at = Some(Instant::now());
                    active.accepted_action = action;
                }
            } else {
                self.finish(Ok(Outcome::Cancelled));
            }
        }
        self.connection
            .flush()
            .map_err(|error| format!("could not flush X11 drag state: {error}"))
    }

    fn update_target(&mut self, target: Option<Window>, x: i16, y: i16) -> Result<(), String> {
        let previous = self.active.as_ref().and_then(|active| active.target);
        if target != previous {
            if let Some(previous) = previous {
                self.send_client(previous, self.atoms.xdnd_leave, [self.source, 0, 0, 0, 0])?;
            }
            if let Some(active) = self.active.as_mut() {
                active.target = target;
                active.accepted = false;
            }
            if let Some(target) = target {
                self.send_client(
                    target,
                    self.atoms.xdnd_enter,
                    [
                        self.source,
                        5 << 24,
                        self.atoms.uri_list,
                        self.atoms.private_type,
                        0,
                    ],
                )?;
            }
        }
        let Some(target) = target else {
            return Ok(());
        };
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.accepted && active.last_position == Some((target, x, y)))
        {
            return Ok(());
        }
        if let Some(active) = self.active.as_mut() {
            active.last_position = Some((target, x, y));
        }
        let desired = self
            .active
            .as_ref()
            .map_or(Action::Copy, |active| active.desired_action);
        let position = (u32::from(x as u16) << 16) | u32::from(y as u16);
        self.send_client(
            target,
            self.atoms.xdnd_position,
            [
                self.source,
                0,
                position,
                CURRENT_TIME,
                self.action_atom(desired),
            ],
        )
    }

    fn target_at_pointer(&self) -> Result<Option<Window>, String> {
        let mut window = self.root;
        let mut target = None;
        loop {
            let pointer = self
                .connection
                .query_pointer(window)
                .map_err(|error| format!("could not inspect the X11 drop target: {error}"))?
                .reply()
                .map_err(|error| format!("could not inspect the X11 drop target: {error}"))?;
            if pointer.child == Window::from(AtomEnum::NONE) {
                break;
            }
            window = pointer.child;
            if window != self.origin && window != self.icon && self.is_aware(window)? {
                target = Some(window);
            }
        }
        Ok(target)
    }

    fn is_aware(&self, window: Window) -> Result<bool, String> {
        self.connection
            .get_property(false, window, self.atoms.xdnd_aware, AtomEnum::ATOM, 0, 1)
            .map_err(|error| format!("could not inspect XdndAware: {error}"))?
            .reply()
            .map(|reply| reply.format == 32 && reply.value_len > 0)
            .map_err(|error| format!("could not inspect XdndAware: {error}"))
    }

    fn handle_event(&mut self, event: Event) -> Result<(), String> {
        match event {
            Event::ClientMessage(event) if event.type_ == self.atoms.xdnd_status => {
                let data = event.data.as_data32();
                if let Some(active) = self.active.as_mut()
                    && active.target == Some(data[0])
                {
                    active.accepted = data[1] & 1 != 0;
                    active.accepted_action =
                        action_from_atom(data[4], active.desired_action, &self.atoms);
                }
            }
            Event::ClientMessage(event) if event.type_ == self.atoms.xdnd_finished => {
                let data = event.data.as_data32();
                if self
                    .active
                    .as_ref()
                    .is_some_and(|active| active.target == Some(data[0]))
                {
                    let accepted = data[1] & 1 != 0;
                    let fallback = self
                        .active
                        .as_ref()
                        .map_or(Action::Copy, |active| active.accepted_action);
                    let action = action_from_atom(data[2], fallback, &self.atoms);
                    self.finish(Ok(if accepted {
                        Outcome::Dropped(action)
                    } else {
                        Outcome::Cancelled
                    }));
                }
            }
            Event::SelectionRequest(event) if event.selection == self.atoms.xdnd_selection => {
                self.answer_selection(event)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn answer_selection(&self, event: SelectionRequestEvent) -> Result<(), String> {
        let property = if event.property == Atom::from(AtomEnum::NONE) {
            event.target
        } else {
            event.property
        };
        let accepted = if event.target == self.atoms.targets {
            self.connection
                .change_property32(
                    PropMode::REPLACE,
                    event.requestor,
                    property,
                    AtomEnum::ATOM,
                    &[self.atoms.uri_list, self.atoms.private_type],
                )
                .map_err(|error| format!("could not publish X11 drag targets: {error}"))?;
            true
        } else if matches!(
            event.target,
            target if target == self.atoms.uri_list || target == self.atoms.private_type
        ) {
            if let Some(active) = &self.active {
                self.connection
                    .change_property8(
                        PropMode::REPLACE,
                        event.requestor,
                        property,
                        event.target,
                        &active.payload,
                    )
                    .map_err(|error| format!("could not publish X11 drag data: {error}"))?;
                true
            } else {
                false
            }
        } else {
            false
        };
        self.connection
            .send_event(
                false,
                event.requestor,
                EventMask::NO_EVENT,
                SelectionNotifyEvent {
                    response_type: SELECTION_NOTIFY_EVENT,
                    sequence: 0,
                    time: event.time,
                    requestor: event.requestor,
                    selection: event.selection,
                    target: event.target,
                    property: if accepted {
                        property
                    } else {
                        Atom::from(AtomEnum::NONE)
                    },
                },
            )
            .map_err(|error| format!("could not notify the X11 drop target: {error}"))?;
        self.connection
            .flush()
            .map_err(|error| format!("could not flush X11 drag data: {error}"))
    }

    fn send_client(&self, target: Window, kind: Atom, data: [u32; 5]) -> Result<(), String> {
        self.connection
            .send_event(
                false,
                target,
                EventMask::NO_EVENT,
                ClientMessageEvent::new(32, target, kind, ClientMessageData::from(data)),
            )
            .map_err(|error| format!("could not send an X11 drag message: {error}"))?
            .check()
            .map_err(|error| format!("the X11 drag target rejected a message: {error}"))?;
        Ok(())
    }

    fn action_atom(&self, action: Action) -> Atom {
        match action {
            Action::Copy => self.atoms.action_copy,
            Action::Move => self.atoms.action_move,
        }
    }

    fn read_incoming_action(&self) -> Action {
        let Ok(property) = self.connection.get_property(
            false,
            self.root,
            self.atoms.waddle_action,
            AtomEnum::CARDINAL,
            0,
            2,
        ) else {
            return Action::Copy;
        };
        let Ok(property) = property.reply() else {
            return Action::Copy;
        };
        let Some(mut values) = property.value32() else {
            return Action::Copy;
        };
        let (Some(source), Some(action)) = (values.next(), values.next()) else {
            return Action::Copy;
        };
        let owner = self
            .connection
            .get_selection_owner(self.atoms.xdnd_selection)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map(|reply| reply.owner);
        if owner != Some(source) {
            return Action::Copy;
        }
        action_from_atom(action, Action::Copy, &self.atoms)
    }

    fn finish(&mut self, result: Result<Outcome, String>) {
        let _ = self.connection.unmap_window(self.icon);
        let _ = self.connection.set_selection_owner(
            Window::from(AtomEnum::NONE),
            self.atoms.xdnd_selection,
            CURRENT_TIME,
        );
        let _ = self
            .connection
            .delete_property(self.root, self.atoms.waddle_action);
        let _ = self.connection.flush();
        if let Some(mut active) = self.active.take()
            && let Some(reply) = active.reply.take()
        {
            let _ = reply.send(result);
        }
    }
}

impl Atoms {
    fn intern(connection: &RustConnection) -> Result<Self, String> {
        fn atom(connection: &RustConnection, name: &str) -> Result<Atom, String> {
            connection
                .intern_atom(false, name.as_bytes())
                .map_err(|error| format!("could not intern {name}: {error}"))?
                .reply()
                .map(|reply| reply.atom)
                .map_err(|error| format!("could not intern {name}: {error}"))
        }
        Ok(Self {
            xdnd_aware: atom(connection, "XdndAware")?,
            xdnd_enter: atom(connection, "XdndEnter")?,
            xdnd_position: atom(connection, "XdndPosition")?,
            xdnd_status: atom(connection, "XdndStatus")?,
            xdnd_leave: atom(connection, "XdndLeave")?,
            xdnd_drop: atom(connection, "XdndDrop")?,
            xdnd_finished: atom(connection, "XdndFinished")?,
            xdnd_selection: atom(connection, "XdndSelection")?,
            xdnd_type_list: atom(connection, "XdndTypeList")?,
            xdnd_action_list: atom(connection, "XdndActionList")?,
            action_copy: atom(connection, "XdndActionCopy")?,
            action_move: atom(connection, "XdndActionMove")?,
            targets: atom(connection, "TARGETS")?,
            uri_list: atom(connection, transfer_formats::URI_LIST_MIME)?,
            private_type: atom(connection, transfer_formats::WADDLE_MIME)?,
            waddle_action: atom(connection, "_WADDLE_XDND_ACTION")?,
        })
    }
}

fn action_from_atom(atom: Atom, fallback: Action, atoms: &Atoms) -> Action {
    if atom == atoms.action_move {
        Action::Move
    } else if atom == atoms.action_copy {
        Action::Copy
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::futures::executor;

    #[test]
    fn real_x11_source_negotiates_move_and_serves_a_multi_entry_uri_list() {
        if std::env::var_os("WADDLE_X11_TEST").is_none() {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("one.txt");
        let second = temp.path().join("two words.txt");
        std::fs::write(&first, "one").unwrap();
        std::fs::write(&second, "two").unwrap();

        let (connection, screen_index) = RustConnection::connect(None).unwrap();
        let screen = &connection.setup().roots[screen_index];
        let root = screen.root;
        let root_visual = screen.root_visual;
        let target = connection.generate_id().unwrap();
        connection
            .create_window(
                COPY_DEPTH_FROM_PARENT,
                target,
                root,
                40,
                40,
                180,
                180,
                0,
                WindowClass::INPUT_OUTPUT,
                root_visual,
                &CreateWindowAux::new().override_redirect(1),
            )
            .unwrap();
        let origin = connection.generate_id().unwrap();
        connection
            .create_window(
                COPY_DEPTH_FROM_PARENT,
                origin,
                root,
                320,
                320,
                120,
                120,
                0,
                WindowClass::INPUT_OUTPUT,
                root_visual,
                &CreateWindowAux::new().override_redirect(1),
            )
            .unwrap();
        let atoms = Atoms::intern(&connection).unwrap();
        connection
            .change_property32(
                PropMode::REPLACE,
                target,
                atoms.xdnd_aware,
                AtomEnum::ATOM,
                &[5],
            )
            .unwrap();
        connection.map_window(target).unwrap();
        connection.map_window(origin).unwrap();
        connection
            .warp_pointer(Window::from(AtomEnum::NONE), root, 0, 0, 0, 0, 80, 80)
            .unwrap();
        connection.flush().unwrap();
        let pointer = connection.query_pointer(root).unwrap().reply().unwrap();
        assert_eq!((pointer.root_x, pointer.root_y), (80, 80));
        assert_eq!(pointer.child, target);

        let (payload_sender, payload_receiver) = mpsc::sync_channel(1);
        let target_thread = thread::spawn(move || {
            let started = Instant::now();
            let mut source_window = None;
            loop {
                while let Some(event) = connection.poll_for_event().unwrap() {
                    match event {
                        Event::ClientMessage(event) if event.type_ == atoms.xdnd_position => {
                            let source = event.data.as_data32()[0];
                            source_window = Some(source);
                            connection
                                .send_event(
                                    false,
                                    source,
                                    EventMask::NO_EVENT,
                                    ClientMessageEvent::new(
                                        32,
                                        source,
                                        atoms.xdnd_status,
                                        ClientMessageData::from([
                                            target,
                                            1,
                                            0,
                                            0,
                                            atoms.action_move,
                                        ]),
                                    ),
                                )
                                .unwrap()
                                .check()
                                .unwrap();
                            connection.flush().unwrap();
                        }
                        Event::ClientMessage(event) if event.type_ == atoms.xdnd_drop => {
                            connection
                                .convert_selection(
                                    target,
                                    atoms.xdnd_selection,
                                    atoms.uri_list,
                                    atoms.xdnd_selection,
                                    CURRENT_TIME,
                                )
                                .unwrap()
                                .check()
                                .unwrap();
                            connection.flush().unwrap();
                        }
                        Event::SelectionNotify(event)
                            if event.selection == atoms.xdnd_selection =>
                        {
                            let payload = connection
                                .get_property(
                                    true,
                                    target,
                                    atoms.xdnd_selection,
                                    atoms.uri_list,
                                    0,
                                    u32::MAX,
                                )
                                .unwrap()
                                .reply()
                                .unwrap()
                                .value;
                            let source = source_window.expect("position precedes selection data");
                            connection
                                .send_event(
                                    false,
                                    source,
                                    EventMask::NO_EVENT,
                                    ClientMessageEvent::new(
                                        32,
                                        source,
                                        atoms.xdnd_finished,
                                        ClientMessageData::from([
                                            target,
                                            1,
                                            atoms.action_move,
                                            0,
                                            0,
                                        ]),
                                    ),
                                )
                                .unwrap()
                                .check()
                                .unwrap();
                            connection.flush().unwrap();
                            let _ = payload_sender.send(payload);
                            return;
                        }
                        _ => {}
                    }
                }
                assert!(started.elapsed() < Duration::from_secs(12));
                thread::sleep(Duration::from_millis(5));
            }
        });

        let source = Source::attach_window(origin).unwrap();
        let completion = source
            .start_inner(
                vec![first.clone(), second.clone()],
                Preview {
                    icon: include_bytes!("../ui/icons/file.svg"),
                    count: 2,
                    copy: false,
                    background: [20, 30, 40, 255],
                    icon_color: [220, 230, 240, 255],
                    accent: [40, 140, 220, 255],
                    badge_text: [255, 255, 255, 255],
                },
                false,
                true,
            )
            .unwrap();
        let release_source = source.clone();
        assert_eq!(source.incoming_action(), Action::Move);
        let release_thread = thread::spawn(move || {
            thread::sleep(Duration::from_millis(1_200));
            release_source.release_synthetic_hold();
        });
        assert_eq!(
            executor::block_on(completion).unwrap(),
            Outcome::Dropped(Action::Move)
        );
        let payload = payload_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        let paths = transfer_formats::decode_uri_list(&payload).unwrap();
        assert_eq!(paths, [first, second]);
        target_thread.join().unwrap();
        release_thread.join().unwrap();
        Adapter::shutdown(&source);
    }
}
