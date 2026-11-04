//! Alerts the user would want (a question, a turn that ended with nothing
//! queued, a failed session), held in `AlertHub` for the `FollowAlerts`
//! streams: the server's and each desktop's.
//!
//! The hub keeps the last `MAX_KEPT` alerts, numbered. Each follower, known
//! by its key, has a cursor into them, so it gets each alert once while the
//! node runs, whenever it connects, and a follower that is away or slow
//! holds nobody else up. A follower never seen starts at the oldest alert
//! kept; one that fell behind the oldest resumes from it.

use crate::lock;
use beep_proto::node::agent::Alert;
use iroh::EndpointId;
use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
};
use tokio::sync::Notify;

/// The most alerts kept; the oldest go.
const MAX_KEPT: usize = 50;
/// The most followers whose place is remembered; the least recently seen
/// goes first.
const MAX_FOLLOWERS: usize = 64;

#[derive(Debug, Default)]
pub(crate) struct AlertHub {
    held: Mutex<Held>,
    /// Woken when an alert joins the ring.
    arrived: Notify,
}

#[derive(Debug, Default)]
struct Held {
    /// The number the next alert gets.
    next: u64,
    kept: VecDeque<Numbered>,
    cursors: HashMap<EndpointId, Cursor>,
    /// Counts reads, to tell which follower was seen least recently.
    clock: u64,
    /// Where `drain` left off.
    #[cfg(test)]
    drained: u64,
}

#[derive(Debug)]
struct Numbered {
    number: u64,
    alert: Alert,
}

#[derive(Debug)]
struct Cursor {
    /// The number of the next alert the follower has not had.
    next: u64,
    seen: u64,
}

/// One stream's view of the alerts.
pub(crate) struct Follow<'a> {
    hub: &'a AlertHub,
    id: EndpointId,
}

impl AlertHub {
    /// Keeps `alert` for every follower to read.
    pub(crate) fn record(&self, alert: Alert) {
        let mut held = lock(&self.held);
        let number = held.next;
        held.next += 1;
        held.kept.push_back(Numbered { number, alert });
        while held.kept.len() > MAX_KEPT {
            held.kept.pop_front();
        }
        drop(held);
        self.arrived.notify_waiters();
    }

    /// The stream of follower `id`, which carries on from where `id` left off.
    pub(crate) fn follow(&self, id: EndpointId) -> Follow<'_> {
        Follow { hub: self, id }
    }

    /// Every alert recorded since the last call, oldest first.
    #[cfg(test)]
    pub(crate) fn drain(&self) -> Vec<Alert> {
        let mut held = lock(&self.held);
        let from = held.drained;
        held.drained = held.next;
        held.kept
            .iter()
            .filter(|kept| kept.number >= from)
            .map(|kept| kept.alert.clone())
            .collect()
    }
}

impl Held {
    /// The first alert follower `id` has not had, with its number.
    fn after(&self, id: EndpointId) -> Option<&Numbered> {
        let from = self.cursors.get(&id).map_or(0, |cursor| cursor.next);
        self.kept.iter().find(|kept| kept.number >= from)
    }

    /// Notes that `id` has had everything up to `number`.
    fn had(&mut self, id: EndpointId, number: u64) {
        self.clock += 1;
        let cursor = Cursor {
            next: number + 1,
            seen: self.clock,
        };
        if self.cursors.len() >= MAX_FOLLOWERS && !self.cursors.contains_key(&id) {
            let oldest = self
                .cursors
                .iter()
                .min_by_key(|(_, cursor)| cursor.seen)
                .map(|(id, _)| *id);
            if let Some(oldest) = oldest {
                self.cursors.remove(&oldest);
            }
        }
        self.cursors.insert(id, cursor);
    }
}

impl Follow<'_> {
    /// The next alert this follower has not had, once there is one. It is
    /// the same alert until `sent` says it went out.
    pub(crate) async fn next(&self) -> Alert {
        loop {
            // Made before looking, so an alert that comes between is not missed.
            let arrived = self.hub.arrived.notified();
            if let Some(kept) = lock(&self.hub.held).after(self.id) {
                return kept.alert.clone();
            }
            arrived.await;
        }
    }

    /// The alert `next` gave went out: the follower has had it.
    pub(crate) fn sent(&self) {
        let mut held = lock(&self.hub.held);
        if let Some(number) = held.after(self.id).map(|kept| kept.number) {
            held.had(self.id, number);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::node::agent::AlertKind;
    use iroh::SecretKey;
    use std::time::Duration;
    use tokio::time::timeout;

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

    fn follower() -> EndpointId {
        SecretKey::generate().public()
    }

    /// The next alert, taken as a stream does: read, then sent.
    async fn take(follow: &Follow<'_>) -> String {
        let alert = timeout(Duration::from_secs(10), follow.next())
            .await
            .expect("an alert is there");
        follow.sent();
        alert.session
    }

    async fn nothing(follow: &Follow<'_>) -> bool {
        timeout(Duration::from_millis(50), follow.next())
            .await
            .is_err()
    }

    #[tokio::test]
    async fn an_offline_desktop_gets_what_it_missed() {
        let hub = AlertHub::default();
        let (server, desktop) = (hub.follow(follower()), follower());
        hub.record(alert("one"));
        assert_eq!(take(&server).await, "one");
        // The desktop was never connected, and the server was.
        hub.record(alert("two"));
        assert_eq!(take(&server).await, "two");
        let desktop = hub.follow(desktop);
        assert_eq!(take(&desktop).await, "one");
        assert_eq!(take(&desktop).await, "two");
    }

    #[tokio::test]
    async fn the_server_and_a_desktop_each_get_every_alert() {
        let hub = AlertHub::default();
        let (server, desktop) = (hub.follow(follower()), hub.follow(follower()));
        for session in ["a", "b", "c"] {
            hub.record(alert(session));
        }
        for follow in [&server, &desktop] {
            for session in ["a", "b", "c"] {
                assert_eq!(take(follow).await, session);
            }
            assert!(nothing(follow).await);
        }
    }

    #[tokio::test]
    async fn a_reconnecting_follower_gets_nothing_twice() {
        let hub = AlertHub::default();
        let id = follower();
        hub.record(alert("a"));
        hub.record(alert("b"));
        let first = hub.follow(id);
        assert_eq!(take(&first).await, "a");
        // A new stream of the same follower carries on, and an alert read
        // but never sent comes again.
        let again = hub.follow(id);
        let unsent = again.next().await;
        assert_eq!(unsent.session, "b");
        let again = hub.follow(id);
        assert_eq!(take(&again).await, "b");
        assert!(nothing(&again).await);
        hub.record(alert("c"));
        assert_eq!(take(&again).await, "c");
    }

    #[tokio::test]
    async fn a_slow_follower_holds_nobody_up() {
        let hub = AlertHub::default();
        let (slow, quick) = (hub.follow(follower()), hub.follow(follower()));
        for n in 0..MAX_KEPT + 10 {
            hub.record(alert(&n.to_string()));
            assert_eq!(take(&quick).await, n.to_string());
        }
        // The slow one fell off the ring and resumes from the oldest kept.
        assert_eq!(take(&slow).await, "10");
    }

    #[test]
    fn only_the_most_recent_followers_are_remembered() {
        let hub = AlertHub::default();
        let ids: Vec<EndpointId> = (0..=MAX_FOLLOWERS).map(|_| follower()).collect();
        hub.record(alert("a"));
        for id in &ids {
            let follow = hub.follow(*id);
            follow.sent();
        }
        assert_eq!(lock(&hub.held).cursors.len(), MAX_FOLLOWERS);
        assert!(!lock(&hub.held).cursors.contains_key(&ids[0]));
    }
}
