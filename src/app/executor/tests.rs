use super::TaskExecutor;
use std::{
    sync::{Arc, Condvar, Mutex, mpsc},
    time::Duration,
};

#[test]
fn executes_all_queued_jobs() {
    let executor = TaskExecutor::new("test", 2);
    let completed = Arc::new((Mutex::new(0usize), Condvar::new()));

    for _ in 0..12 {
        let completed = completed.clone();
        assert!(executor.execute(move || {
            let (count, ready) = &*completed;
            let mut count = count.lock().unwrap();
            *count += 1;
            ready.notify_one();
        }));
    }

    let (count, ready) = &*completed;
    let count = count.lock().unwrap();
    let (count, timeout) = ready
        .wait_timeout_while(count, Duration::from_secs(2), |count| *count < 12)
        .unwrap();
    assert!(!timeout.timed_out());
    assert_eq!(*count, 12);
}

#[test]
fn a_blocked_job_does_not_starve_the_second_worker() {
    let executor = TaskExecutor::new("parallel-test", 2);
    let (release_sender, release_receiver) = mpsc::channel::<()>();
    let (started_sender, started_receiver) = mpsc::channel::<()>();
    let (quick_sender, quick_receiver) = mpsc::channel::<()>();

    assert!(executor.execute(move || {
        started_sender.send(()).unwrap();
        release_receiver.recv().unwrap();
    }));
    started_receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    assert!(executor.execute(move || {
        let _ = quick_sender.send(());
    }));

    quick_receiver
        .recv_timeout(Duration::from_millis(250))
        .expect("the free worker should execute the quick job");
    release_sender.send(()).unwrap();
}

#[test]
fn independent_executors_do_not_starve_each_other() {
    let background = TaskExecutor::new("blocked-lane", 1);
    let navigation = TaskExecutor::new("navigation-lane", 1);
    let (release_sender, release_receiver) = mpsc::channel::<()>();
    let (started_sender, started_receiver) = mpsc::channel::<()>();
    let (navigation_sender, navigation_receiver) = mpsc::channel::<()>();

    assert!(background.execute(move || {
        started_sender.send(()).unwrap();
        release_receiver.recv().unwrap();
    }));
    started_receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    assert!(navigation.execute(move || {
        let _ = navigation_sender.send(());
    }));

    navigation_receiver
        .recv_timeout(Duration::from_millis(250))
        .expect("background I/O must not block navigation");
    release_sender.send(()).unwrap();
}
