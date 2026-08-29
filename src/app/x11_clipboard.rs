use std::{
    collections::{HashMap, VecDeque},
    hash::{Hash, Hasher},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc as std_mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use iced::{
    Subscription,
    futures::{StreamExt, channel::mpsc},
};
use tokio::sync::oneshot;
use x11rb::{
    COPY_DEPTH_FROM_PARENT, CURRENT_TIME,
    connection::{Connection, RequestConnection},
    protocol::{
        Event,
        xproto::{
            Atom, AtomEnum, ChangeWindowAttributesAux, ConnectionExt, CreateWindowAux, EventMask,
            PropMode, Property, SELECTION_NOTIFY_EVENT, SelectionNotifyEvent, Window, WindowClass,
        },
    },
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

use crate::{
    transfer::{
        ClipboardAdapter, ClipboardCompletion, ClipboardImport, ClipboardPayload,
        Event as TransferEvent,
    },
    transfer_formats::{self, EncodedOffer},
};

const POLL_INTERVAL: Duration = Duration::from_millis(5);
const READ_TIMEOUT: Duration = Duration::from_secs(2);
static NEXT_SOURCE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub(super) struct Source(Arc<SourceInner>);

struct SourceInner {
    id: u64,
    commands: std_mpsc::Sender<Command>,
    events: Mutex<Option<mpsc::UnboundedReceiver<TransferEvent>>>,
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
            .debug_struct("X11ClipboardSource")
            .finish_non_exhaustive()
    }
}

impl Source {
    pub(super) fn attach() -> Result<Self, String> {
        let (commands, receiver) = std_mpsc::channel();
        let (events_sender, events_receiver) = mpsc::unbounded();
        let (ready_sender, ready_receiver) = std_mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("waddle-x11-clipboard".to_owned())
            .spawn(move || {
                let result = Worker::connect(events_sender).and_then(|mut worker| {
                    let _ = ready_sender.send(Ok(()));
                    worker.run(receiver)
                });
                if let Err(error) = result {
                    let _ = ready_sender.send(Err(error));
                }
            })
            .map_err(|error| format!("could not start the X11 clipboard worker: {error}"))?;
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
                Err(format!("X11 clipboard initialization timed out: {error}"))
            }
        }
    }

    pub(super) fn subscription(&self) -> Subscription<TransferEvent> {
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

    fn request<T>(
        &self,
        build: impl FnOnce(std_mpsc::SyncSender<Result<T, String>>) -> Command,
    ) -> Result<T, String> {
        let (reply, receiver) = std_mpsc::sync_channel(1);
        self.0
            .commands
            .send(build(reply))
            .map_err(|_| "the X11 clipboard worker has stopped".to_owned())?;
        receiver
            .recv_timeout(READ_TIMEOUT)
            .map_err(|error| format!("X11 clipboard request timed out: {error}"))?
    }
}

impl ClipboardAdapter for Source {
    fn write_clipboard(&self, payload: ClipboardPayload) -> Result<(), String> {
        let generation = payload.generation;
        let offer = transfer_formats::encode(&payload)?;
        self.request(|reply| Command::Write {
            offer,
            generation,
            reply,
        })
    }

    fn read_clipboard(&self) -> Result<ClipboardCompletion, String> {
        let (reply, receiver) = oneshot::channel();
        self.0
            .commands
            .send(Command::Read { reply })
            .map_err(|_| "the X11 clipboard worker has stopped".to_owned())?;
        Ok(Box::pin(async move {
            receiver
                .await
                .unwrap_or_else(|_| Err("the X11 clipboard worker stopped unexpectedly".to_owned()))
        }))
    }

    fn clear_clipboard(&self, generation: u64) {
        let _ = self.0.commands.send(Command::Clear { generation });
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
    Write {
        offer: EncodedOffer,
        generation: u64,
        reply: std_mpsc::SyncSender<Result<(), String>>,
    },
    Read {
        reply: oneshot::Sender<Result<ClipboardImport, String>>,
    },
    Clear {
        generation: u64,
    },
    Shutdown,
}

struct Atoms {
    clipboard: Atom,
    property: Atom,
    targets: Atom,
    incr: Atom,
    atom_by_mime: HashMap<&'static str, Atom>,
}

struct ActiveOffer {
    generation: u64,
    data: HashMap<Atom, Vec<u8>>,
}

struct IncrementalSend {
    requestor: Window,
    property: Atom,
    target: Atom,
    data: Vec<u8>,
    position: usize,
}

enum ReadStage {
    Targets,
    Data {
        target: Atom,
        mime: &'static str,
    },
    Incremental {
        target: Atom,
        mime: &'static str,
        data: Vec<u8>,
    },
}

struct PendingRead {
    owner: Window,
    started: Instant,
    stage: ReadStage,
    pending: VecDeque<(Atom, &'static str)>,
    entries: Vec<(&'static str, Vec<u8>)>,
    reply: oneshot::Sender<Result<ClipboardImport, String>>,
}

struct Worker {
    connection: RustConnection,
    window: Window,
    atoms: Atoms,
    events: mpsc::UnboundedSender<TransferEvent>,
    active: Option<ActiveOffer>,
    sends: HashMap<(Window, Atom), IncrementalSend>,
    read: Option<PendingRead>,
    max_chunk: usize,
}

impl Worker {
    fn connect(events: mpsc::UnboundedSender<TransferEvent>) -> Result<Self, String> {
        let (connection, screen) = RustConnection::connect(None)
            .map_err(|error| format!("could not connect to X11: {error}"))?;
        let window = connection
            .generate_id()
            .map_err(|error| format!("could not allocate an X11 window: {error}"))?;
        let root = connection
            .setup()
            .roots
            .get(screen)
            .ok_or_else(|| "the X11 screen is unavailable".to_owned())?;
        connection
            .create_window(
                COPY_DEPTH_FROM_PARENT,
                window,
                root.root,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_OUTPUT,
                root.root_visual,
                &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
            )
            .map_err(|error| format!("could not create the X11 clipboard window: {error}"))?
            .check()
            .map_err(|error| format!("could not create the X11 clipboard window: {error}"))?;
        let atoms = Atoms::new(&connection)?;
        let max_chunk = connection
            .maximum_request_bytes()
            .saturating_sub(128)
            .clamp(4_000, 64 * 1024);
        Ok(Self {
            connection,
            window,
            atoms,
            events,
            active: None,
            sends: HashMap::new(),
            read: None,
            max_chunk,
        })
    }

    fn run(&mut self, commands: std_mpsc::Receiver<Command>) -> Result<(), String> {
        loop {
            while let Ok(command) = commands.try_recv() {
                match command {
                    Command::Write {
                        offer,
                        generation,
                        reply,
                    } => {
                        let _ = reply.send(self.write(offer, generation));
                    }
                    Command::Read { reply } => self.begin_read(reply),
                    Command::Clear { generation } => self.clear(generation),
                    Command::Shutdown => return Ok(()),
                }
            }
            while let Some(event) = self
                .connection
                .poll_for_event()
                .map_err(|error| format!("X11 clipboard event failed: {error}"))?
            {
                self.handle_event(event)?;
            }
            if self
                .read
                .as_ref()
                .is_some_and(|read| read.started.elapsed() >= READ_TIMEOUT)
            {
                self.finish_read(Err("the X11 clipboard read timed out".to_owned()));
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    fn write(&mut self, offer: EncodedOffer, generation: u64) -> Result<(), String> {
        let data = offer
            .into_entries()
            .into_iter()
            .filter_map(|(mime, data)| {
                self.atoms
                    .atom_by_mime
                    .get(mime)
                    .copied()
                    .map(|atom| (atom, data))
            })
            .collect();
        self.active = Some(ActiveOffer { generation, data });
        self.connection
            .set_selection_owner(self.window, self.atoms.clipboard, CURRENT_TIME)
            .map_err(|error| format!("could not own the X11 clipboard: {error}"))?
            .check()
            .map_err(|error| format!("could not own the X11 clipboard: {error}"))?;
        let owner = self
            .connection
            .get_selection_owner(self.atoms.clipboard)
            .map_err(|error| format!("could not inspect the X11 clipboard owner: {error}"))?
            .reply()
            .map_err(|error| format!("could not inspect the X11 clipboard owner: {error}"))?
            .owner;
        if owner == self.window {
            Ok(())
        } else {
            self.active = None;
            Err("X11 refused clipboard ownership".to_owned())
        }
    }

    fn clear(&mut self, generation: u64) {
        if self
            .active
            .as_ref()
            .is_some_and(|offer| offer.generation == generation)
        {
            let _ = self.connection.set_selection_owner(
                Window::from(AtomEnum::NONE),
                self.atoms.clipboard,
                CURRENT_TIME,
            );
            let _ = self.connection.flush();
            self.active = None;
        }
    }

    fn begin_read(&mut self, reply: oneshot::Sender<Result<ClipboardImport, String>>) {
        if self.read.is_some() {
            let _ = reply.send(Err("another X11 clipboard read is active".to_owned()));
            return;
        }
        let owner = match self.connection.get_selection_owner(self.atoms.clipboard) {
            Ok(cookie) => match cookie.reply() {
                Ok(reply) if reply.owner != Window::from(AtomEnum::NONE) => reply.owner,
                _ => {
                    let _ = reply.send(Err("the X11 clipboard has no owner".to_owned()));
                    return;
                }
            },
            Err(error) => {
                let _ = reply.send(Err(format!("could not inspect the X11 clipboard: {error}")));
                return;
            }
        };
        self.read = Some(PendingRead {
            owner,
            started: Instant::now(),
            stage: ReadStage::Targets,
            pending: VecDeque::new(),
            entries: Vec::new(),
            reply,
        });
        if let Err(error) = self.convert(self.atoms.targets) {
            self.finish_read(Err(error));
        }
    }

    fn convert(&self, target: Atom) -> Result<(), String> {
        self.connection
            .convert_selection(
                self.window,
                self.atoms.clipboard,
                target,
                self.atoms.property,
                CURRENT_TIME,
            )
            .map_err(|error| format!("could not request the X11 clipboard: {error}"))?
            .check()
            .map_err(|error| format!("could not request the X11 clipboard: {error}"))?;
        self.connection
            .flush()
            .map_err(|error| format!("could not flush the X11 clipboard request: {error}"))
    }

    fn handle_event(&mut self, event: Event) -> Result<(), String> {
        match event {
            Event::SelectionRequest(event) => self.serve(event)?,
            Event::SelectionNotify(event) if event.requestor == self.window => {
                self.receive_notify(event.property)?;
            }
            Event::PropertyNotify(event)
                if event.state == Property::DELETE
                    && self.sends.contains_key(&(event.window, event.atom)) =>
            {
                self.send_increment(event.window, event.atom)?;
            }
            Event::PropertyNotify(event)
                if event.window == self.window && event.state == Property::NEW_VALUE =>
            {
                self.receive_increment()?;
            }
            Event::SelectionClear(event) if event.selection == self.atoms.clipboard => {
                if self.active.take().is_some() {
                    let _ = self
                        .events
                        .unbounded_send(TransferEvent::ClipboardOwnershipLost);
                }
                self.sends.clear();
            }
            _ => {}
        }
        Ok(())
    }

    fn serve(
        &mut self,
        event: x11rb::protocol::xproto::SelectionRequestEvent,
    ) -> Result<(), String> {
        let property = if event.property == Atom::from(AtomEnum::NONE) {
            event.target
        } else {
            event.property
        };
        let mut accepted = false;
        if let Some(active) = &self.active {
            if event.target == self.atoms.targets {
                let mut targets = vec![self.atoms.targets];
                targets.extend(active.data.keys().copied());
                self.connection
                    .change_property32(
                        PropMode::REPLACE,
                        event.requestor,
                        property,
                        AtomEnum::ATOM,
                        &targets,
                    )
                    .map_err(|error| format!("could not publish X11 clipboard targets: {error}"))?;
                accepted = true;
            } else if let Some(data) = active.data.get(&event.target) {
                if data.len() <= self.max_chunk {
                    self.connection
                        .change_property8(
                            PropMode::REPLACE,
                            event.requestor,
                            property,
                            event.target,
                            data,
                        )
                        .map_err(|error| {
                            format!("could not publish X11 clipboard data: {error}")
                        })?;
                } else {
                    self.connection
                        .change_window_attributes(
                            event.requestor,
                            &ChangeWindowAttributesAux::new()
                                .event_mask(EventMask::PROPERTY_CHANGE),
                        )
                        .map_err(|error| format!("could not start X11 INCR transfer: {error}"))?;
                    self.connection
                        .change_property32(
                            PropMode::REPLACE,
                            event.requestor,
                            property,
                            self.atoms.incr,
                            &[u32::try_from(data.len()).unwrap_or(u32::MAX)],
                        )
                        .map_err(|error| format!("could not start X11 INCR transfer: {error}"))?;
                    self.sends.insert(
                        (event.requestor, property),
                        IncrementalSend {
                            requestor: event.requestor,
                            property,
                            target: event.target,
                            data: data.clone(),
                            position: 0,
                        },
                    );
                }
                accepted = true;
            }
        }
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
            .map_err(|error| format!("could not notify the X11 clipboard requestor: {error}"))?;
        self.connection
            .flush()
            .map_err(|error| format!("could not flush X11 clipboard data: {error}"))
    }

    fn send_increment(&mut self, requestor: Window, property: Atom) -> Result<(), String> {
        let Some(state) = self.sends.get_mut(&(requestor, property)) else {
            return Ok(());
        };
        let (chunk, end, finished) = next_increment(&state.data, state.position, self.max_chunk);
        self.connection
            .change_property8(
                PropMode::REPLACE,
                state.requestor,
                state.property,
                state.target,
                chunk,
            )
            .map_err(|error| format!("could not continue X11 INCR transfer: {error}"))?;
        state.position = end;
        if finished {
            self.sends.remove(&(requestor, property));
        }
        self.connection
            .flush()
            .map_err(|error| format!("could not flush X11 INCR data: {error}"))
    }

    fn receive_notify(&mut self, property: Atom) -> Result<(), String> {
        if property == Atom::from(AtomEnum::NONE) {
            self.finish_read(Err(
                "the X11 clipboard does not support file targets".to_owned()
            ));
            return Ok(());
        }
        let Some(read) = self.read.as_mut() else {
            return Ok(());
        };
        match read.stage {
            ReadStage::Targets => {
                let reply = self
                    .connection
                    .get_property(
                        true,
                        self.window,
                        self.atoms.property,
                        AtomEnum::ATOM,
                        0,
                        u32::MAX,
                    )
                    .map_err(|error| format!("could not read X11 clipboard targets: {error}"))?
                    .reply()
                    .map_err(|error| format!("could not read X11 clipboard targets: {error}"))?;
                let offered = reply
                    .value32()
                    .map(Iterator::collect::<Vec<_>>)
                    .unwrap_or_default();
                let mut targets = [
                    transfer_formats::WADDLE_MIME,
                    transfer_formats::GNOME_MIME,
                    transfer_formats::URI_LIST_MIME,
                    transfer_formats::KDE_CUT_MIME,
                ]
                .into_iter()
                .filter_map(|mime| {
                    self.atoms
                        .atom_by_mime
                        .get(mime)
                        .copied()
                        .filter(|atom| offered.contains(atom))
                        .map(|atom| (atom, mime))
                })
                .collect::<VecDeque<_>>();
                if !targets.iter().any(|(_, mime)| {
                    matches!(
                        *mime,
                        transfer_formats::WADDLE_MIME
                            | transfer_formats::GNOME_MIME
                            | transfer_formats::URI_LIST_MIME
                    )
                }) {
                    self.finish_read(Err(
                        "the X11 clipboard does not contain local files".to_owned()
                    ));
                    return Ok(());
                }
                let (target, mime) = targets.pop_front().expect("file target checked above");
                read.pending = targets;
                read.stage = ReadStage::Data { target, mime };
                self.convert(target)?;
            }
            ReadStage::Data { target, mime } => {
                let reply = self
                    .connection
                    .get_property(
                        false,
                        self.window,
                        self.atoms.property,
                        AtomEnum::NONE,
                        0,
                        u32::MAX,
                    )
                    .map_err(|error| format!("could not read X11 clipboard data: {error}"))?
                    .reply()
                    .map_err(|error| format!("could not read X11 clipboard data: {error}"))?;
                if reply.type_ == self.atoms.incr {
                    self.connection
                        .delete_property(self.window, self.atoms.property)
                        .map_err(|error| {
                            format!("could not start reading X11 INCR data: {error}")
                        })?;
                    read.stage = ReadStage::Incremental {
                        target,
                        mime,
                        data: Vec::new(),
                    };
                    self.connection.flush().map_err(|error| {
                        format!("could not start reading X11 INCR data: {error}")
                    })?;
                } else if reply.type_ != target {
                    self.finish_read(Err(
                        "the X11 clipboard returned an unexpected type".to_owned()
                    ));
                } else {
                    self.finish_format(mime, reply.value);
                }
            }
            ReadStage::Incremental { .. } => {}
        }
        Ok(())
    }

    fn receive_increment(&mut self) -> Result<(), String> {
        let Some(read) = self.read.as_mut() else {
            return Ok(());
        };
        let ReadStage::Incremental { target, mime, data } = &mut read.stage else {
            return Ok(());
        };
        let reply = self
            .connection
            .get_property(
                true,
                self.window,
                self.atoms.property,
                AtomEnum::NONE,
                0,
                u32::MAX,
            )
            .map_err(|error| format!("could not read X11 INCR data: {error}"))?
            .reply()
            .map_err(|error| format!("could not read X11 INCR data: {error}"))?;
        if reply.type_ != *target {
            return Ok(());
        }
        if reply.value.is_empty() {
            let mime = *mime;
            let data = std::mem::take(data);
            self.finish_format(mime, data);
        } else if data.len().saturating_add(reply.value.len()) > transfer_formats::MAX_BYTES {
            self.finish_read(Err(
                "the X11 clipboard payload is larger than 4 MiB".to_owned()
            ));
        } else {
            data.extend_from_slice(&reply.value);
        }
        Ok(())
    }

    fn finish_format(&mut self, mime: &'static str, data: Vec<u8>) {
        let same_owner = self.read.as_ref().is_some_and(|read| {
            self.connection
                .get_selection_owner(self.atoms.clipboard)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .is_some_and(|owner| owner.owner == read.owner)
        });
        if !same_owner {
            self.finish_read(Err(
                "the X11 clipboard changed while Waddle was reading it".to_owned()
            ));
            return;
        }
        let next = self.read.as_mut().and_then(|read| {
            read.entries.push((mime, data));
            read.pending.pop_front()
        });
        if let Some((target, mime)) = next {
            if let Some(read) = self.read.as_mut() {
                read.stage = ReadStage::Data { target, mime };
            }
            if let Err(error) = self.convert(target) {
                self.finish_read(Err(error));
            }
            return;
        }
        let result = self.read.as_ref().map_or_else(
            || Err("the X11 clipboard read disappeared".to_owned()),
            |read| {
                let entries = read
                    .entries
                    .iter()
                    .map(|(mime, data)| (*mime, data.as_slice()))
                    .collect::<Vec<_>>();
                transfer_formats::decode_offer(&entries)
            },
        );
        self.finish_read(result);
    }

    fn finish_read(&mut self, result: Result<ClipboardImport, String>) {
        if let Some(read) = self.read.take() {
            let _ = read.reply.send(result);
        }
    }
}

fn next_increment(data: &[u8], position: usize, max_chunk: usize) -> (&[u8], usize, bool) {
    if position >= data.len() {
        (&[], data.len(), true)
    } else {
        let end = position.saturating_add(max_chunk).min(data.len());
        (&data[position..end], end, false)
    }
}

impl Atoms {
    fn new(connection: &RustConnection) -> Result<Self, String> {
        let intern = |name: &str| -> Result<Atom, String> {
            connection
                .intern_atom(false, name.as_bytes())
                .map_err(|error| format!("could not intern X11 atom {name}: {error}"))?
                .reply()
                .map(|reply| reply.atom)
                .map_err(|error| format!("could not intern X11 atom {name}: {error}"))
        };
        let clipboard = intern("CLIPBOARD")?;
        let property = intern("WADDLE_CLIPBOARD")?;
        let targets = intern("TARGETS")?;
        let incr = intern("INCR")?;
        let mut atom_by_mime = HashMap::new();
        for mime in [
            transfer_formats::WADDLE_MIME,
            transfer_formats::GNOME_MIME,
            transfer_formats::URI_LIST_MIME,
            transfer_formats::KDE_CUT_MIME,
        ] {
            let atom = intern(mime)?;
            atom_by_mime.insert(mime, atom);
        }
        Ok(Self {
            clipboard,
            property,
            targets,
            incr,
            atom_by_mime,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transfer::Action;

    #[test]
    fn incremental_chunks_end_with_the_required_empty_property() {
        let data = b"abcdefghij";
        let mut position = 0;

        let (first, next, done) = next_increment(data, position, 4);
        assert_eq!(first, b"abcd");
        assert!(!done);
        position = next;
        let (second, next, done) = next_increment(data, position, 4);
        assert_eq!(second, b"efgh");
        assert!(!done);
        position = next;
        let (third, next, done) = next_increment(data, position, 4);
        assert_eq!(third, b"ij");
        assert!(!done);
        position = next;
        let (last, _, done) = next_increment(data, position, 4);
        assert!(last.is_empty());
        assert!(done);
    }

    #[test]
    fn two_x11_adapters_exchange_a_large_multi_entry_offer() {
        if std::env::var_os("WADDLE_X11_TEST").is_none() {
            return;
        }
        let writer = Source::attach().expect("writer");
        let reader = Source::attach().expect("reader");
        let paths = (0..5_000)
            .map(|index| std::path::PathBuf::from(format!("/tmp/waddle-{index:04}")))
            .collect::<Vec<_>>();

        ClipboardAdapter::write_clipboard(
            &writer,
            ClipboardPayload {
                paths: paths.clone(),
                action: Action::Copy,
                generation: 7,
            },
        )
        .expect("publish");
        let completion = ClipboardAdapter::read_clipboard(&reader).expect("read request");
        let imported = iced::futures::executor::block_on(completion).expect("read clipboard");

        assert_eq!(imported.paths, paths);
        assert_eq!(imported.action, Action::Copy);
        assert_eq!(imported.generation, Some(7));

        ClipboardAdapter::write_clipboard(
            &writer,
            ClipboardPayload {
                paths: paths.clone(),
                action: Action::Move,
                generation: 8,
            },
        )
        .expect("publish cut");
        let completion = ClipboardAdapter::read_clipboard(&reader).expect("cut read request");
        let imported = iced::futures::executor::block_on(completion).expect("read cut clipboard");
        assert_eq!(imported.paths, paths);
        assert_eq!(imported.action, Action::Move);
        assert_eq!(imported.generation, Some(8));
    }
}
