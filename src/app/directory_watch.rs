use std::{
    collections::{HashMap, HashSet},
    ffi::CString,
    os::{
        fd::RawFd,
        unix::ffi::{OsStrExt, OsStringExt},
    },
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc as std_mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use iced::{
    Subscription,
    futures::{StreamExt, channel::mpsc},
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
const DEBOUNCE: Duration = Duration::from_millis(120);
const MAX_WATCHES: usize = 2_048;

#[derive(Clone, Debug)]
pub(super) struct Event {
    pub(super) path: PathBuf,
    pub(super) removed: Vec<PathBuf>,
    pub(super) watch_failed: bool,
}

#[derive(Default)]
struct PendingChange {
    changed: Option<Instant>,
    removed: HashSet<PathBuf>,
}

#[derive(Clone)]
pub(super) struct Source(Arc<Inner>);

struct Inner {
    id: u64,
    commands: std_mpsc::Sender<Command>,
    events: Mutex<Option<mpsc::UnboundedReceiver<Event>>>,
}

enum Command {
    Watch(Vec<PathBuf>),
    Shutdown,
}

impl Source {
    pub(super) fn new() -> Result<Self, String> {
        let (commands, command_receiver) = std_mpsc::channel();
        let (events, event_receiver) = mpsc::unbounded();
        let worker = thread::Builder::new()
            .name("waddle-directory-watch".to_owned())
            .spawn(move || worker(command_receiver, events))
            .map_err(|error| format!("could not start directory monitor: {error}"))?;
        drop(worker);
        Ok(Self(Arc::new(Inner {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            commands,
            events: Mutex::new(Some(event_receiver)),
        })))
    }

    pub(super) fn watch_many(&self, paths: impl IntoIterator<Item = PathBuf>) -> bool {
        let paths = paths.into_iter().collect::<Vec<_>>();
        let overflow = paths.len() > MAX_WATCHES;
        let _ = self.0.commands.send(Command::Watch(paths));
        overflow
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
}

impl std::hash::Hash for Source {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.id.hash(state);
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
    }
}

fn worker(commands: std_mpsc::Receiver<Command>, events: mpsc::UnboundedSender<Event>) {
    // SAFETY: inotify_init1 has no borrowed arguments.
    let descriptor = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
    if descriptor < 0 {
        return;
    }
    let mut watched = HashMap::<i32, PathBuf>::new();
    let mut pending = HashMap::<PathBuf, PendingChange>::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        for command in commands.try_iter() {
            match command {
                Command::Watch(paths) => {
                    if replace_watches(descriptor, &mut watched, paths)
                        && events
                            .unbounded_send(Event {
                                path: PathBuf::new(),
                                removed: Vec::new(),
                                watch_failed: true,
                            })
                            .is_err()
                    {
                        close_descriptor(descriptor);
                        return;
                    }
                }
                Command::Shutdown => {
                    close_descriptor(descriptor);
                    return;
                }
            }
        }
        let mut poll = libc::pollfd {
            fd: descriptor,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: poll points at one initialized pollfd.
        let ready = unsafe { libc::poll(&mut poll, 1, 25) };
        if ready > 0 && poll.revents & libc::POLLIN != 0 {
            // SAFETY: buffer is writable and descriptor is a nonblocking inotify fd.
            let read = unsafe { libc::read(descriptor, buffer.as_mut_ptr().cast(), buffer.len()) };
            if read > 0 {
                collect_changed_watches(&buffer[..read as usize], &watched, &mut pending);
            }
        }
        let ready = pending
            .iter()
            .filter_map(|(path, change)| {
                change
                    .changed
                    .is_some_and(|changed| changed.elapsed() >= DEBOUNCE)
                    .then_some(path.clone())
            })
            .collect::<Vec<_>>();
        for path in ready {
            let removed = pending
                .remove(&path)
                .map(|change| change.removed.into_iter().collect())
                .unwrap_or_default();
            if events
                .unbounded_send(Event {
                    path,
                    removed,
                    watch_failed: false,
                })
                .is_err()
            {
                close_descriptor(descriptor);
                return;
            }
        }
    }
}

fn collect_changed_watches(
    buffer: &[u8],
    watched: &HashMap<i32, PathBuf>,
    pending: &mut HashMap<PathBuf, PendingChange>,
) {
    let mut offset = 0;
    while offset + std::mem::size_of::<libc::inotify_event>() <= buffer.len() {
        // SAFETY: one complete record header is in bounds; unaligned reads are supported here.
        let event = unsafe {
            buffer
                .as_ptr()
                .add(offset)
                .cast::<libc::inotify_event>()
                .read_unaligned()
        };
        let record_size =
            std::mem::size_of::<libc::inotify_event>().saturating_add(event.len as usize);
        if offset.saturating_add(record_size) > buffer.len() {
            break;
        }
        if let Some(directory) = watched.get(&event.wd) {
            let change = pending.entry(directory.clone()).or_default();
            change.changed = Some(Instant::now());
            let name_start = offset + std::mem::size_of::<libc::inotify_event>();
            let name_bytes = &buffer[name_start..offset + record_size];
            let name_length = name_bytes
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(name_bytes.len());
            if name_length > 0 && event.mask & (libc::IN_MOVED_FROM | libc::IN_DELETE) != 0 {
                let name = std::ffi::OsString::from_vec(name_bytes[..name_length].to_vec());
                change.removed.insert(directory.join(name));
            }
        }
        offset = offset.saturating_add(record_size);
    }
}

fn replace_watches(
    descriptor: RawFd,
    watched: &mut HashMap<i32, PathBuf>,
    paths: Vec<PathBuf>,
) -> bool {
    let mut desired = HashSet::new();
    let desired_order = paths
        .into_iter()
        .filter(|path| desired.insert(path.clone()))
        .take(MAX_WATCHES)
        .collect::<Vec<_>>();
    desired = desired_order.iter().cloned().collect();
    let removed = watched
        .iter()
        .filter_map(|(watch, path)| (!desired.contains(path)).then_some(*watch))
        .collect::<Vec<_>>();
    for watch in removed {
        // SAFETY: watch was returned by inotify_add_watch for this descriptor.
        unsafe { libc::inotify_rm_watch(descriptor, watch) };
        watched.remove(&watch);
    }
    let mask = libc::IN_ATTRIB
        | libc::IN_CLOSE_WRITE
        | libc::IN_CREATE
        | libc::IN_DELETE
        | libc::IN_DELETE_SELF
        | libc::IN_MODIFY
        | libc::IN_MOVE_SELF
        | libc::IN_MOVED_FROM
        | libc::IN_MOVED_TO;
    let current = watched.values().cloned().collect::<HashSet<_>>();
    let mut failed = false;
    for path in desired_order.iter().filter(|path| !current.contains(*path)) {
        let Ok(path_bytes) = CString::new(path.as_os_str().as_bytes()) else {
            continue;
        };
        // SAFETY: path_bytes is a valid C string for this call.
        let watch = unsafe { libc::inotify_add_watch(descriptor, path_bytes.as_ptr(), mask) };
        if watch >= 0 {
            watched.insert(watch, path.clone());
        } else {
            failed = true;
        }
    }
    failed
}

fn close_descriptor(descriptor: RawFd) {
    // SAFETY: the worker owns this descriptor and closes it once on exit.
    unsafe { libc::close(descriptor) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inotify_source_debounces_a_burst_for_the_watched_directory() {
        let temp = tempfile::tempdir().unwrap();
        let source = Source::new().unwrap();
        source.watch_many([temp.path().to_path_buf()]);
        thread::sleep(Duration::from_millis(30));
        for index in 0..5 {
            std::fs::write(temp.path().join(format!("file-{index}")), "x").unwrap();
        }
        let mut events = source.0.events.lock().unwrap().take().unwrap();
        let event = iced::futures::executor::block_on(events.next()).expect("debounced event");
        assert_eq!(event.path, temp.path());
        assert!(event.removed.is_empty());
        assert!(!event.watch_failed);
        thread::sleep(Duration::from_millis(180));
        assert!(events.try_recv().is_err());
    }

    #[test]
    fn inotify_source_keeps_multiple_watched_directories() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        let source = Source::new().unwrap();
        source.watch_many([first.clone(), second.clone()]);
        thread::sleep(Duration::from_millis(30));
        std::fs::write(first.join("one"), "x").unwrap();
        std::fs::write(second.join("two"), "x").unwrap();

        let mut events = source.0.events.lock().unwrap().take().unwrap();
        let mut paths = [
            iced::futures::executor::block_on(events.next())
                .unwrap()
                .path,
            iced::futures::executor::block_on(events.next())
                .unwrap()
                .path,
        ];
        paths.sort();
        assert_eq!(paths, [first, second]);
    }

    #[test]
    fn inotify_distinguishes_delete_internal_rename_and_move_out() {
        let temp = tempfile::tempdir().unwrap();
        let watched = temp.path().join("watched");
        let other_watched = temp.path().join("other-watched");
        let outside = temp.path().join("outside");
        for directory in [&watched, &other_watched, &outside] {
            std::fs::create_dir(directory).unwrap();
        }
        let deleted = watched.join("deleted");
        let internal = watched.join("internal");
        let moved = watched.join("moved");
        for path in [&deleted, &internal, &moved] {
            std::fs::write(path, "x").unwrap();
        }
        let source = Source::new().unwrap();
        source.watch_many([watched.clone(), other_watched.clone()]);
        thread::sleep(Duration::from_millis(30));
        std::fs::remove_file(&deleted).unwrap();
        std::fs::rename(&internal, other_watched.join("internal")).unwrap();
        std::fs::rename(&moved, outside.join("moved")).unwrap();

        let mut events = source.0.events.lock().unwrap().take().unwrap();
        let first = iced::futures::executor::block_on(events.next()).unwrap();
        let second = iced::futures::executor::block_on(events.next()).unwrap();
        let watched_event = [first, second]
            .into_iter()
            .find(|event| event.path == watched)
            .unwrap();
        let mut removed = watched_event.removed;
        removed.sort();
        assert_eq!(removed, [deleted, internal, moved]);
    }
}
