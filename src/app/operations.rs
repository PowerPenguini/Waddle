use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use tokio::sync::Semaphore;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Kind {
    Navigation,
    Details,
    Search,
    Background,
    Mutation,
    Command,
}

#[derive(Clone, Debug)]
pub(super) struct Cancellation {
    generation: Option<(Arc<AtomicU64>, u64)>,
}

impl Cancellation {
    pub(super) fn is_cancelled(&self) -> bool {
        self.generation
            .as_ref()
            .is_some_and(|(current, mine)| current.load(Ordering::Acquire) != *mine)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum Completion<T> {
    Finished(Result<T, String>),
    Cancelled,
}

pub(super) type Job<T> = Pin<Box<dyn Future<Output = Completion<T>> + Send>>;

#[derive(Debug)]
pub(super) struct ForegroundActivity {
    active: Arc<AtomicUsize>,
}

impl Drop for ForegroundActivity {
    fn drop(&mut self) {
        let previous = self.active.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "foreground operation count underflowed");
    }
}

#[derive(Clone, Debug)]
pub(super) struct Operations {
    navigation: Arc<Semaphore>,
    background: Arc<Semaphore>,
    mutation: Arc<Semaphore>,
    navigation_generation: Arc<AtomicU64>,
    details_generation: Arc<AtomicU64>,
    search_generation: Arc<AtomicU64>,
    foreground_active: Arc<AtomicUsize>,
}

impl Default for Operations {
    fn default() -> Self {
        Self {
            navigation: Arc::new(Semaphore::new(2)),
            background: Arc::new(Semaphore::new(2)),
            mutation: Arc::new(Semaphore::new(1)),
            navigation_generation: Arc::new(AtomicU64::new(0)),
            details_generation: Arc::new(AtomicU64::new(0)),
            search_generation: Arc::new(AtomicU64::new(0)),
            foreground_active: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl Operations {
    pub(super) fn cancel(&self, kind: Kind) {
        if let Some(generation) = self.generation(kind) {
            generation.fetch_add(1, Ordering::AcqRel);
        }
    }

    pub(super) fn run<T, F>(&self, kind: Kind, work: F) -> Job<T>
    where
        T: Send + 'static,
        F: FnOnce(Cancellation) -> Result<T, String> + Send + 'static,
    {
        self.schedule(kind, None, None, work)
    }

    pub(super) fn run_foreground<T, F>(&self, kind: Kind, work: F) -> Job<T>
    where
        T: Send + 'static,
        F: FnOnce(Cancellation) -> Result<T, String> + Send + 'static,
    {
        let activity = self.begin_foreground();
        self.schedule(kind, None, Some(activity), work)
    }

    pub(super) fn run_after<T, F>(&self, kind: Kind, delay: Duration, work: F) -> Job<T>
    where
        T: Send + 'static,
        F: FnOnce(Cancellation) -> Result<T, String> + Send + 'static,
    {
        self.schedule(kind, Some(delay), None, work)
    }

    pub(super) fn begin_foreground(&self) -> ForegroundActivity {
        self.foreground_active.fetch_add(1, Ordering::AcqRel);
        ForegroundActivity {
            active: Arc::clone(&self.foreground_active),
        }
    }

    pub(super) fn foreground_active(&self) -> bool {
        self.foreground_active.load(Ordering::Acquire) > 0
    }

    fn schedule<T, F>(
        &self,
        kind: Kind,
        delay: Option<Duration>,
        foreground: Option<ForegroundActivity>,
        work: F,
    ) -> Job<T>
    where
        T: Send + 'static,
        F: FnOnce(Cancellation) -> Result<T, String> + Send + 'static,
    {
        let lane = self.lane(kind);
        let cancellation = self.begin(kind);
        Box::pin(async move {
            let _foreground = foreground;
            if let Some(delay) = delay {
                tokio::time::sleep(delay).await;
                if cancellation.is_cancelled() {
                    return Completion::Cancelled;
                }
            }
            let permit = match lane.acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => {
                    return Completion::Finished(Err("operation queue closed".to_owned()));
                }
            };
            if cancellation.is_cancelled() {
                return Completion::Cancelled;
            }
            let worker_cancellation = cancellation.clone();
            let result = tokio::task::spawn_blocking(move || work(worker_cancellation))
                .await
                .map_err(|error| format!("background task failed: {error}"))
                .and_then(std::convert::identity);
            drop(permit);
            if cancellation.is_cancelled() {
                Completion::Cancelled
            } else {
                Completion::Finished(result)
            }
        })
    }

    fn begin(&self, kind: Kind) -> Cancellation {
        let generation = self.generation(kind).map(|generation| {
            let mine = generation.fetch_add(1, Ordering::AcqRel) + 1;
            (generation, mine)
        });
        Cancellation { generation }
    }

    fn generation(&self, kind: Kind) -> Option<Arc<AtomicU64>> {
        match kind {
            Kind::Navigation => Some(Arc::clone(&self.navigation_generation)),
            Kind::Details => Some(Arc::clone(&self.details_generation)),
            Kind::Search => Some(Arc::clone(&self.search_generation)),
            Kind::Background | Kind::Mutation | Kind::Command => None,
        }
    }

    fn lane(&self, kind: Kind) -> Arc<Semaphore> {
        match kind {
            Kind::Navigation => Arc::clone(&self.navigation),
            Kind::Details | Kind::Search | Kind::Background => Arc::clone(&self.background),
            Kind::Mutation | Kind::Command => Arc::clone(&self.mutation),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        thread,
        time::Duration,
    };

    use super::*;

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
    }

    #[test]
    fn latest_work_receives_cancellation_and_cannot_complete() {
        runtime().block_on(async {
            let operations = Operations::default();
            let first_operations = operations.clone();
            let (started_sender, started_receiver) = mpsc::sync_channel(1);
            let first = tokio::spawn(first_operations.run(Kind::Search, move |cancellation| {
                started_sender.send(()).unwrap();
                while !cancellation.is_cancelled() {
                    thread::yield_now();
                }
                Ok(1)
            }));
            tokio::task::yield_now().await;
            started_receiver
                .recv_timeout(Duration::from_secs(1))
                .unwrap();

            let second = operations.run(Kind::Search, |_| Ok(2)).await;
            assert_eq!(second, Completion::Finished(Ok(2)));
            assert_eq!(first.await.unwrap(), Completion::Cancelled);
        });
    }

    #[test]
    fn command_and_mutation_share_one_serial_lane() {
        runtime().block_on(async {
            let operations = Operations::default();
            let active = Arc::new(AtomicUsize::new(0));
            let maximum = Arc::new(AtomicUsize::new(0));
            let run = |kind| {
                let operations = operations.clone();
                let active = Arc::clone(&active);
                let maximum = Arc::clone(&maximum);
                tokio::spawn(operations.run(kind, move |_| {
                    let current = active.fetch_add(1, Ordering::AcqRel) + 1;
                    maximum.fetch_max(current, Ordering::AcqRel);
                    thread::sleep(Duration::from_millis(15));
                    active.fetch_sub(1, Ordering::AcqRel);
                    Ok(())
                }))
            };

            let first = run(Kind::Mutation);
            let second = run(Kind::Command);
            assert_eq!(first.await.unwrap(), Completion::Finished(Ok(())));
            assert_eq!(second.await.unwrap(), Completion::Finished(Ok(())));
            assert_eq!(maximum.load(Ordering::Acquire), 1);
        });
    }

    #[test]
    fn foreground_activity_survives_overlapping_work() {
        let operations = Operations::default();
        let first = operations.run_foreground(Kind::Background, |_| Ok(()));
        let second = operations.run_foreground(Kind::Mutation, |_| Ok(()));

        assert!(operations.foreground_active());
        drop(first);
        assert!(operations.foreground_active());
        drop(second);
        assert!(!operations.foreground_active());
    }

    #[test]
    fn replaced_foreground_work_cannot_mark_newer_work_inactive() {
        runtime().block_on(async {
            let operations = Operations::default();
            let (first_started_sender, first_started_receiver) = mpsc::sync_channel(1);
            let first_operations = operations.clone();
            let first = tokio::spawn(first_operations.run_foreground(
                Kind::Details,
                move |cancellation| {
                    first_started_sender.send(()).unwrap();
                    while !cancellation.is_cancelled() {
                        thread::yield_now();
                    }
                    Ok(())
                },
            ));
            tokio::task::yield_now().await;
            first_started_receiver
                .recv_timeout(Duration::from_secs(1))
                .unwrap();

            let (second_release_sender, second_release_receiver) = mpsc::sync_channel(1);
            let second_operations = operations.clone();
            let second = tokio::spawn(second_operations.run_foreground(Kind::Details, move |_| {
                second_release_receiver.recv().unwrap();
                Ok(())
            }));

            assert_eq!(first.await.unwrap(), Completion::Cancelled);
            assert!(operations.foreground_active());
            second_release_sender.send(()).unwrap();
            assert_eq!(second.await.unwrap(), Completion::Finished(Ok(())));
            assert!(!operations.foreground_active());
        });
    }
}
