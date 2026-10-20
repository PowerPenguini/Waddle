use std::{
    future::Future,
    sync::{OnceLock, mpsc},
    thread,
    time::Duration,
};

use gio::glib::MainContext;

type Job = Box<dyn FnOnce(&gio::VolumeMonitor, &MainContext) + Send>;

// GVolumeMonitor is a process-wide singleton tied to the global default context.
// Keep it, its objects, and all their callbacks on one persistent desktop thread.
fn submit(job: Job) -> Result<(), String> {
    static OWNER: OnceLock<Result<mpsc::Sender<Job>, String>> = OnceLock::new();
    OWNER
        .get_or_init(start_owner)
        .as_ref()
        .map_err(Clone::clone)?
        .send(job)
        .map_err(|_| "Desktop volume monitor stopped".to_owned())
}

fn start_owner() -> Result<mpsc::Sender<Job>, String> {
    let (sender, receiver) = mpsc::channel::<Job>();
    let (ready, initialized) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("waddle-desktop-volumes".to_owned())
        .spawn(move || {
            let context = MainContext::default();
            let _ownership = match context.acquire() {
                Ok(ownership) => ownership,
                Err(error) => {
                    let _ = ready.send(Err(error.to_string()));
                    return;
                }
            };
            let monitor = gio::VolumeMonitor::get();
            let _ = ready.send(Ok(()));
            loop {
                // Bound dispatch so a busy desktop cannot starve new queries.
                for _ in 0..32 {
                    if !context.pending() {
                        break;
                    }
                    context.iteration(false);
                }
                match receiver.recv_timeout(Duration::from_millis(10)) {
                    Ok(job) => job(&monitor, &context),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
        })
        .map_err(|error| error.to_string())?;
    initialized
        .recv()
        .map_err(|_| "Desktop volume monitor failed to start".to_owned())??;
    Ok(sender)
}

pub(super) fn query<T: Send + 'static>(
    query: impl FnOnce(&gio::VolumeMonitor) -> T + Send + 'static,
) -> Result<T, String> {
    let (reply, result) = mpsc::sync_channel(1);
    submit(Box::new(move |monitor, _| {
        let _ = reply.send(query(monitor));
    }))?;
    result
        .recv()
        .map_err(|_| "Desktop volume query stopped".to_owned())
}

pub(super) fn operate<T, F>(
    operation: impl FnOnce(gio::VolumeMonitor) -> F + Send + 'static,
) -> Result<T, String>
where
    T: Send + 'static,
    F: Future<Output = Result<T, String>> + 'static,
{
    let (reply, result) = mpsc::sync_channel(1);
    submit(Box::new(move |monitor, context| {
        let future = operation(monitor.clone());
        context.spawn_local(async move {
            let _ = reply.send(future.await);
        });
    }))?;
    result
        .recv()
        .map_err(|_| "Desktop volume operation stopped".to_owned())?
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;

    #[test]
    fn pending_volume_operations_do_not_block_queries_or_change_context() {
        let (started, running) = mpsc::sync_channel(1);
        let (release, released) = iced::futures::channel::oneshot::channel();
        let operation = thread::spawn(move || {
            operate(move |monitor| async move {
                let local = Rc::new(thread::current().id());
                assert!(MainContext::thread_default().is_none());
                started.send(*local).unwrap();
                released.await.map_err(|error| error.to_string())?;
                assert_eq!(*local, thread::current().id());
                assert!(MainContext::thread_default().is_none());
                drop(monitor);
                Ok(*local)
            })
        });
        let owner = running.recv_timeout(Duration::from_secs(2)).unwrap();
        let (queried, result) = mpsc::sync_channel(1);
        let query = thread::spawn(move || {
            let result = super::query(|_| {
                assert!(MainContext::thread_default().is_none());
                thread::current().id()
            });
            let _ = queried.send(result);
        });
        let response = result.recv_timeout(Duration::from_secs(2));
        // Release even if the query timed out, so a failed test can clean up.
        release.send(()).unwrap();
        assert_eq!(operation.join().unwrap().unwrap(), owner);
        query.join().unwrap();
        assert_eq!(response.unwrap().unwrap(), owner);
    }
}
