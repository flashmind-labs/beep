use super::{Quitting, StopGuard, StopOutcome, Stopped};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

#[test]
fn deferred_quit_wakes_waiter_and_allows_next_exit() {
    let quitting = Arc::new(Quitting::default());
    let starts = Arc::new(AtomicUsize::new(0));
    let (ready_tx, ready_rx) = mpsc::channel();

    let starts_run = Arc::clone(&starts);
    let deferred = quitting.defer_or_exit_now(
        Duration::from_secs(5),
        move |stopped| {
            starts_run.fetch_add(1, Ordering::SeqCst);
            let mut guard = StopGuard::new(stopped);
            guard.succeed();
        },
        move || {
            let _ = ready_tx.send(());
        },
    );
    assert!(deferred);
    ready_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("waiter runs when stop completes");

    let deferred_again = quitting.defer_or_exit_now(
        Duration::from_secs(5),
        |_| panic!("stop must not run twice"),
        || panic!("waiter must not register after ready"),
    );
    assert!(!deferred_again);
    assert!(quitting.finish(Duration::from_secs(5), |_| panic!("already stopped")));
    assert!(!quitting.finish(Duration::from_secs(5), |_| panic!("finish runs once")));
    assert_eq!(starts.load(Ordering::SeqCst), 1);
}

#[test]
fn stalled_runtime_times_out_on_os_clock_without_blocking() {
    let quitting = Arc::new(Quitting::default());
    let (ready_tx, ready_rx) = mpsc::channel();
    let held = Arc::new(std::sync::Mutex::new(None));
    let held_slot = Arc::clone(&held);

    // Simulates every Tokio worker wedged in WebKit's callOnMainRunLoopAndWait:
    // `Stopped` is never signaled, yet the OS waiter still unblocks at `limit`.
    let deferred = quitting.defer_or_exit_now(
        Duration::from_millis(20),
        move |stopped| {
            *held_slot.lock().unwrap() = Some(stopped);
        },
        move || {
            let _ = ready_tx.send(());
        },
    );
    assert!(deferred);
    ready_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("OS deadline unblocks even when async runtime never polls");
    assert!(quitting.finish(Duration::from_secs(1), |_| panic!("already timed out")));
}

#[test]
fn concurrent_quit_requests_share_one_stop_and_notify_every_waiter() {
    let quitting = Arc::new(Quitting::default());
    let starts = Arc::new(AtomicUsize::new(0));
    let (stop_tx, stop_rx) = mpsc::channel();
    let (first_tx, first_rx) = mpsc::channel();
    let (second_tx, second_rx) = mpsc::channel();

    let starts_first = Arc::clone(&starts);
    assert!(quitting.defer_or_exit_now(
        Duration::from_secs(5),
        move |stopped| {
            starts_first.fetch_add(1, Ordering::SeqCst);
            let _ = stop_tx.send(stopped);
        },
        move || {
            let _ = first_tx.send(());
        },
    ));

    assert!(quitting.defer_or_exit_now(
        Duration::from_secs(5),
        |_| panic!("second caller must share the in-flight stop"),
        move || {
            let _ = second_tx.send(());
        },
    ));

    let stopped = stop_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    stopped.finish(Ok::<(), &str>(()));

    first_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    second_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(starts.load(Ordering::SeqCst), 1);
}

#[test]
fn direct_finish_during_stopping_drops_pending_waiters() {
    let quitting = Arc::new(Quitting::default());
    let (stop_tx, stop_rx) = mpsc::channel();
    let (waiter_tx, waiter_rx) = mpsc::channel();

    assert!(quitting.defer_or_exit_now(
        Duration::from_secs(5),
        move |stopped| {
            let _ = stop_tx.send(stopped);
        },
        move || {
            let _ = waiter_tx.send(());
        },
    ));

    let stopped = stop_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let release = Arc::clone(&stopped);
    let signal = std::thread::spawn(move || {
        release.finish(Ok::<(), &str>(()));
    });

    assert!(quitting.finish(Duration::from_secs(1), |_| panic!("already started")));
    signal.join().unwrap();
    assert_eq!(stopped.wait(Instant::now()), StopOutcome::Done);
    assert!(waiter_rx.try_recv().is_err());
}

#[test]
fn dropped_stop_guard_reports_failure_instead_of_hanging() {
    let stopped = Stopped::new();
    {
        let _guard = StopGuard::new(Arc::clone(&stopped));
    }
    assert_eq!(
        stopped.wait(Instant::now() + Duration::from_secs(5)),
        StopOutcome::Failed
    );
}
