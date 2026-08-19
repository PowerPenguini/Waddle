use std::{
    sync::{Arc, Mutex, mpsc},
    thread,
};

type Job = Box<dyn FnOnce() + Send + 'static>;

#[derive(Clone)]
pub(super) struct TaskExecutor {
    sender: mpsc::Sender<Job>,
}

impl TaskExecutor {
    pub(super) fn new(name: &'static str, worker_count: usize) -> Self {
        assert!(worker_count > 0, "an executor needs at least one worker");
        let (sender, receiver) = mpsc::channel::<Job>();
        let receiver = Arc::new(Mutex::new(receiver));

        for index in 0..worker_count {
            let receiver = receiver.clone();
            thread::Builder::new()
                .name(format!("polarexp-{name}-{index}"))
                .spawn(move || worker_loop(receiver))
                .expect("failed to start a PolarExp I/O worker");
        }

        Self { sender }
    }

    pub(super) fn execute(&self, job: impl FnOnce() + Send + 'static) -> bool {
        self.sender.send(Box::new(job)).is_ok()
    }
}

fn worker_loop(receiver: Arc<Mutex<mpsc::Receiver<Job>>>) {
    loop {
        let job = {
            let receiver = receiver.lock().unwrap();
            receiver.recv()
        };
        let Ok(job) = job else {
            break;
        };
        job();
    }
}

#[cfg(test)]
mod tests;
