use std::{
    collections::{HashMap, HashSet},
    ffi::CString,
    os::{fd::RawFd, unix::ffi::OsStrExt},
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

#[derive(Clone, Debug)]
pub(super) struct Event {
    pub(super) path: PathBuf,
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
            .name("polarexp-directory-watch".to_owned())
            .spawn(move || worker(command_receiver, events))
            .map_err(|error| format!("could not start directory monitor: {error}"))?;
        drop(worker);
        Ok(Self(Arc::new(Inner {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            commands,
            events: Mutex::new(Some(event_receiver)),
        })))
    }

    pub(super) fn watch_many(&self, paths: impl IntoIterator<Item = PathBuf>) {
        let _ = self
            .0
            .commands
            .send(Command::Watch(paths.into_iter().collect()));
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
    let mut pending = HashMap::<PathBuf, Instant>::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        for command in commands.try_iter() {
            match command {
                Command::Watch(paths) => replace_watches(descriptor, &mut watched, paths),
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
            .filter_map(|(path, changed)| (changed.elapsed() >= DEBOUNCE).then_some(path.clone()))
            .collect::<Vec<_>>();
        for path in ready {
            pending.remove(&path);
            if events.unbounded_send(Event { path }).is_err() {
                close_descriptor(descriptor);
                return;
            }
        }
    }
}

fn collect_changed_watches(
    buffer: &[u8],
    watched: &HashMap<i32, PathBuf>,
    pending: &mut HashMap<PathBuf, Instant>,
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
        if let Some(path) = watched.get(&event.wd) {
            pending.insert(path.clone(), Instant::now());
        }
        offset = offset
            .saturating_add(std::mem::size_of::<libc::inotify_event>())
            .saturating_add(event.len as usize);
    }
}

fn replace_watches(descriptor: RawFd, watched: &mut HashMap<i32, PathBuf>, paths: Vec<PathBuf>) {
    let desired = paths.into_iter().collect::<HashSet<_>>();
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
    for path in desired.difference(&current) {
        let Ok(path_bytes) = CString::new(path.as_os_str().as_bytes()) else {
            continue;
        };
        // SAFETY: path_bytes is a valid C string for this call.
        let watch = unsafe { libc::inotify_add_watch(descriptor, path_bytes.as_ptr(), mask) };
        if watch >= 0 {
            watched.insert(watch, path.clone());
        }
    }
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
}
