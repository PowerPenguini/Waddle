use std::{
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
    Watch(PathBuf),
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

    pub(super) fn watch(&self, path: PathBuf) {
        let _ = self.0.commands.send(Command::Watch(path));
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
    let mut watched: Option<(i32, PathBuf)> = None;
    let mut pending: Option<(PathBuf, Instant)> = None;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        for command in commands.try_iter() {
            match command {
                Command::Watch(path) => replace_watch(descriptor, &mut watched, path),
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
            if read > 0
                && let Some((_, path)) = &watched
            {
                pending = Some((path.clone(), Instant::now()));
            }
        }
        if pending
            .as_ref()
            .is_some_and(|(_, changed)| changed.elapsed() >= DEBOUNCE)
            && let Some((path, _)) = pending.take()
            && events.unbounded_send(Event { path }).is_err()
        {
            close_descriptor(descriptor);
            return;
        }
    }
}

fn replace_watch(descriptor: RawFd, watched: &mut Option<(i32, PathBuf)>, path: PathBuf) {
    if watched
        .as_ref()
        .is_some_and(|(_, current)| current == &path)
    {
        return;
    }
    if let Some((watch, _)) = watched.take() {
        // SAFETY: watch was returned by inotify_add_watch for this descriptor.
        unsafe { libc::inotify_rm_watch(descriptor, watch) };
    }
    let Ok(path_bytes) = CString::new(path.as_os_str().as_bytes()) else {
        return;
    };
    let mask = libc::IN_ATTRIB
        | libc::IN_CLOSE_WRITE
        | libc::IN_CREATE
        | libc::IN_DELETE
        | libc::IN_DELETE_SELF
        | libc::IN_MODIFY
        | libc::IN_MOVE_SELF
        | libc::IN_MOVED_FROM
        | libc::IN_MOVED_TO;
    // SAFETY: path_bytes is a valid C string for this call.
    let watch = unsafe { libc::inotify_add_watch(descriptor, path_bytes.as_ptr(), mask) };
    if watch >= 0 {
        *watched = Some((watch, path));
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
        source.watch(temp.path().to_path_buf());
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
}
