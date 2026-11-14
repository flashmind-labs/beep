//! Reads of `tokio::sync::watch` values that never hand out the lock.
//!
//! A watch keeps its value behind a writer-preferring `RwLock`. A thread
//! that reads it again while it still holds a read waits behind any write
//! queued in between, and that write waits on the first read: the thread
//! stops for good, and with it whatever locks it holds (#686). Every read
//! goes through here, as a closure over the value, so no guard outlives
//! the read. A read started inside another on the same thread fails debug
//! builds and tests. Clippy refuses the watch's own `borrow`,
//! `borrow_and_update` and `wait_for` elsewhere.
#![allow(clippy::disallowed_methods)]

use std::cell::Cell;

use tokio::sync::watch::{Receiver, Sender, error::RecvError};

thread_local! {
    /// Whether this thread is inside a read.
    static READING: Cell<bool> = const { Cell::new(false) };
}

/// `f` of the current value.
pub fn read<T, R>(watch: &Receiver<T>, f: impl FnOnce(&T) -> R) -> R {
    let _reading = Reading::start();
    f(&watch.borrow())
}

/// `f` of the current value, marked seen.
pub fn read_and_update<T, R>(watch: &mut Receiver<T>, f: impl FnOnce(&T) -> R) -> R {
    let _reading = Reading::start();
    f(&watch.borrow_and_update())
}

/// `f` of the value last sent.
pub fn read_sent<T, R>(watch: &Sender<T>, f: impl FnOnce(&T) -> R) -> R {
    let _reading = Reading::start();
    f(&watch.borrow())
}

/// Waits until `ready` holds for the value, or the sender is gone.
pub async fn wait_for<T>(
    watch: &mut Receiver<T>,
    mut ready: impl FnMut(&T) -> bool,
) -> Result<(), RecvError> {
    watch
        .wait_for(|value| {
            let _reading = Reading::start();
            ready(value)
        })
        .await
        .map(drop)
}

/// A read in progress on this thread.
struct Reading {
    was: bool,
}

impl Reading {
    fn start() -> Self {
        let was = READING.replace(true);
        debug_assert!(
            !was,
            "a watch read started inside another on the same thread; it waits forever once a write queues in between (#686)"
        );
        Self { was }
    }
}

impl Drop for Reading {
    fn drop(&mut self) {
        READING.set(self.was);
    }
}

#[cfg(test)]
#[path = "tests/watched.rs"]
mod tests;
