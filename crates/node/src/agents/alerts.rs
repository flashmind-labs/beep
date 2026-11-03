//! What the node's sessions did that the user would want to hear about,
//! waiting for the server's `FollowAlerts` stream: a question, a turn that
//! ended with nothing queued behind it, a session that failed. Each waits
//! in a bounded outbox, so alerts from while the server was away still
//! reach it, the oldest dropped first.

use beep_proto::node::agent::Alert;
use std::{
    collections::VecDeque,
    sync::{Mutex, MutexGuard, PoisonError},
};
use tokio::sync::Notify;

/// The most alerts waiting for the server; the oldest go.
const MAX_UNSENT: usize = 50;

#[derive(Debug, Default)]
pub(crate) struct AlertHub {
    unsent: Mutex<VecDeque<Alert>>,
    /// Woken when an alert joins `unsent`.
    arrived: Notify,
}

impl AlertHub {
    fn unsent(&self) -> MutexGuard<'_, VecDeque<Alert>> {
        self.unsent.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Has `alert` wait for the server.
    pub(crate) fn record(&self, alert: Alert) {
        let mut unsent = self.unsent();
        unsent.push_back(alert);
        while unsent.len() > MAX_UNSENT {
            unsent.pop_front();
        }
        drop(unsent);
        self.arrived.notify_one();
    }

    /// The oldest alert waiting for the server, once there is one.
    pub(crate) async fn next(&self) -> Alert {
        loop {
            let arrived = self.arrived.notified();
            if let Some(alert) = self.unsent().pop_front() {
                return alert;
            }
            arrived.await;
        }
    }

    /// Every alert waiting, oldest first, taken out.
    #[cfg(test)]
    pub(crate) fn drain(&self) -> Vec<Alert> {
        self.unsent().drain(..).collect()
    }
}
