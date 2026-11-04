//! Alerts the user would want (a question, a turn that ended with nothing
//! queued, a failed session), held in `AlertHub` for the `FollowAlerts`
//! streams: the server's and each desktop's.
//!
//! Every open stream gets every alert. While none is open the newest wait,
//! and the first stream to open takes them.

use crate::lock;
use beep_proto::node::agent::Alert;
use std::{collections::VecDeque, future::pending, mem::take, sync::Mutex};
use tokio::sync::broadcast::{self, error::RecvError};

/// The most alerts waiting for a stream; the oldest go.
const MAX_UNSENT: usize = 50;

#[derive(Debug)]
pub(crate) struct AlertHub {
    held: Mutex<Held>,
    sent: broadcast::Sender<Alert>,
}

#[derive(Debug, Default)]
struct Held {
    /// Streams open now.
    following: usize,
    /// What came while `following` was zero.
    unsent: VecDeque<Alert>,
}

impl Default for AlertHub {
    fn default() -> Self {
        Self {
            held: Mutex::default(),
            sent: broadcast::channel(MAX_UNSENT).0,
        }
    }
}

/// One stream's view of the alerts, until dropped.
pub(crate) struct Follow<'a> {
    hub: &'a AlertHub,
    backlog: VecDeque<Alert>,
    next: broadcast::Receiver<Alert>,
}

impl AlertHub {
    /// Has `alert` reach every open stream, or wait for the first one.
    pub(crate) fn record(&self, alert: Alert) {
        let mut held = lock(&self.held);
        if held.following == 0 {
            held.unsent.push_back(alert);
            while held.unsent.len() > MAX_UNSENT {
                held.unsent.pop_front();
            }
            return;
        }
        // Sent under the lock, so a stream that opens now misses it or
        // takes it, never both.
        let _ = self.sent.send(alert);
    }

    /// Opens a stream, which takes what was waiting.
    pub(crate) fn follow(&self) -> Follow<'_> {
        let mut held = lock(&self.held);
        held.following += 1;
        Follow {
            hub: self,
            backlog: take(&mut held.unsent),
            next: self.sent.subscribe(),
        }
    }

    /// Every alert waiting, oldest first, taken out.
    #[cfg(test)]
    pub(crate) fn drain(&self) -> Vec<Alert> {
        lock(&self.held).unsent.drain(..).collect()
    }
}

impl Follow<'_> {
    /// The next alert, once there is one.
    pub(crate) async fn next(&mut self) -> Alert {
        if let Some(alert) = self.backlog.pop_front() {
            return alert;
        }
        loop {
            match self.next.recv().await {
                Ok(alert) => return alert,
                // A stream that fell this far behind loses the oldest.
                Err(RecvError::Lagged(_)) => {}
                // The hub owns the sender and outlives its streams.
                Err(RecvError::Closed) => pending().await,
            }
        }
    }
}

impl Drop for Follow<'_> {
    fn drop(&mut self) {
        lock(&self.hub.held).following -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::node::agent::AlertKind;

    fn alert(session: &str) -> Alert {
        Alert {
            session: session.into(),
            agent: "claude".into(),
            title: None,
            cwd: String::new(),
            kind: AlertKind::Failed {
                reason: String::new(),
            },
        }
    }

    #[tokio::test]
    async fn every_open_stream_gets_every_alert_and_the_first_takes_the_backlog() {
        let hub = AlertHub::default();
        hub.record(alert("early"));
        let mut server = hub.follow();
        let mut desktop = hub.follow();
        hub.record(alert("late"));
        assert_eq!(server.next().await.session, "early");
        assert_eq!(server.next().await.session, "late");
        assert_eq!(desktop.next().await.session, "late");
        drop((server, desktop));
        hub.record(alert("alone"));
        assert_eq!(hub.follow().next().await.session, "alone");
    }
}
