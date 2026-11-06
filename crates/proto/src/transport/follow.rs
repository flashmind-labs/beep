//! Keeps an app's events stream open: reconnects when it drops and resumes
//! after the last notice read.

use crate::{
    AppEvent,
    client::{Client, refusal},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{sync::Notify, task::JoinHandle, time::sleep};

/// Waits between attempts after a failure, the last repeating.
const BACKOFF: [Duration; 5] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(30),
];

/// Takes each event but heartbeats and `Resume`.
pub type OnEvent = Arc<dyn Fn(AppEvent) + Send + Sync>;

/// The task following `client`'s events. Dropping it stops the task.
pub struct Follow {
    task: JoinHandle<()>,
    shared: Arc<Shared>,
}

struct Shared {
    open: AtomicBool,
    /// Wakes a wait between attempts, or ends the open stream to start again.
    retry: Notify,
}

impl Follow {
    /// Follows `client`'s events from now on, handing each to `on_event`.
    pub fn start(client: Arc<Client>, on_event: OnEvent) -> Self {
        let shared = Arc::new(Shared {
            open: AtomicBool::new(false),
            retry: Notify::new(),
        });
        let task = tokio::spawn(follow(client, on_event, shared.clone()));
        Self { task, shared }
    }

    /// True while the stream is open.
    pub fn is_open(&self) -> bool {
        self.shared.open.load(Ordering::Relaxed)
    }

    /// Opens the stream again now, as after a network change.
    pub fn reconnect(&self) {
        self.shared.retry.notify_one();
    }
}

impl Drop for Follow {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn follow(client: Arc<Client>, on_event: OnEvent, shared: Arc<Shared>) {
    let mut after = None;
    let mut failures = 0usize;
    loop {
        match client.events(after).await {
            Ok(mut events) => {
                shared.open.store(true, Ordering::Relaxed);
                failures = 0;
                let mut asked = false;
                loop {
                    let next = tokio::select! {
                        next = events.next() => next,
                        () = shared.retry.notified() => {
                            asked = true;
                            break;
                        }
                    };
                    match next {
                        Ok(Some(AppEvent::Heartbeat {})) => {}
                        Ok(Some(AppEvent::Resume { after: from })) => after = Some(from),
                        Ok(Some(event)) => {
                            if let AppEvent::Notice { id, .. } = &event {
                                after = Some(*id);
                            }
                            on_event(event);
                        }
                        Ok(None) => break,
                        Err(e) => {
                            log::debug!("events stream ended: {e:#}");
                            break;
                        }
                    }
                }
                shared.open.store(false, Ordering::Relaxed);
                // A server that ends streams at once is not asked again at once.
                if !asked {
                    tokio::select! {
                        () = sleep(BACKOFF[0]) => {}
                        () = shared.retry.notified() => {}
                    }
                }
            }
            Err(e) => {
                if let Some(refused) = refusal(&e) {
                    log::warn!("events refused: {refused}");
                } else {
                    log::debug!("events stream failed: {e:#}");
                }
                let wait = BACKOFF[failures.min(BACKOFF.len() - 1)];
                failures += 1;
                tokio::select! {
                    () = sleep(wait) => {}
                    () = shared.retry.notified() => {}
                }
            }
        }
    }
}
