//! Alerts the user would want (a question, a turn that ended with nothing
//! queued, a failed session), held in `AlertHub` for the `FollowAlerts`
//! streams: the server's and each desktop's. The CI runs a node tells
//! (`machine::ci`) are held the same way, in a hub of `RunNews`.
//!
//! The hub keeps the last `MAX_KEPT` alerts, numbered. Each follower, known
//! by its key, has a cursor into them, so it gets each alert once while the
//! node runs, whenever it connects, and a follower that is away or slow
//! holds nobody else up. A follower never seen starts at the alerts of the
//! last `FRESH` minutes, so a desktop that joins does not replay old news;
//! one that fell behind the oldest alert kept resumes from it.

use crate::lock;
use beep_proto::node::agent::Alert;
use iroh::EndpointId;
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::Notify;

/// The most alerts kept; the oldest go.
const MAX_KEPT: usize = 50;
/// How far back a follower seen for the first time starts.
const FRESH: Duration = Duration::from_secs(10 * 60);
/// The most followers whose place is remembered; the least recently seen
/// goes first.
const MAX_FOLLOWERS: usize = 64;

#[derive(Debug)]
pub(crate) struct AlertHub<T = Alert> {
    held: Mutex<Held<T>>,
    /// Woken when an alert joins the ring.
    arrived: Notify,
}

impl<T> Default for AlertHub<T> {
    fn default() -> Self {
        Self {
            held: Mutex::new(Held {
                next: 0,
                kept: VecDeque::new(),
                cursors: HashMap::new(),
                clock: 0,
                #[cfg(test)]
                drained: 0,
            }),
            arrived: Notify::new(),
        }
    }
}

#[derive(Debug)]
struct Held<T> {
    /// The number the next alert gets.
    next: u64,
    kept: VecDeque<Numbered<T>>,
    cursors: HashMap<EndpointId, Cursor>,
    /// Counts reads, to tell which follower was seen least recently.
    clock: u64,
    /// Where `drain` left off.
    #[cfg(test)]
    drained: u64,
}

#[derive(Debug)]
struct Numbered<T> {
    number: u64,
    alert: T,
    at: Instant,
}

#[derive(Debug)]
struct Cursor {
    /// The number of the next alert the follower has not had.
    next: u64,
    seen: u64,
}

/// One stream's view of the alerts.
pub(crate) struct Follow<'a, T> {
    hub: &'a AlertHub<T>,
    id: EndpointId,
    /// One more than the number of the alert `next` last gave, or 0.
    given: AtomicU64,
}

impl<T: Clone> AlertHub<T> {
    /// Keeps `alert` for every follower to read.
    pub(crate) fn record(&self, alert: T) {
        self.record_at(alert, Instant::now());
    }

    fn record_at(&self, alert: T, at: Instant) {
        let mut held = lock(&self.held);
        let number = held.next;
        held.next += 1;
        held.kept.push_back(Numbered { number, alert, at });

        while held.kept.len() > MAX_KEPT {
            held.kept.pop_front();
        }

        drop(held);
        self.arrived.notify_waiters();
    }

    /// The stream of follower `id`, which carries on from where `id` left off.
    pub(crate) fn follow(&self, id: EndpointId) -> Follow<'_, T> {
        Follow {
            hub: self,
            id,
            given: AtomicU64::new(0),
        }
    }

    /// Every alert recorded since the last call, oldest first.
    #[cfg(test)]
    pub(crate) fn drain(&self) -> Vec<T> {
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

impl<T> Held<T> {
    /// The first alert follower `id` has not had, with its number.
    fn after(&self, id: EndpointId) -> Option<&Numbered<T>> {
        let from = self.cursors.get(&id).map_or(0, |cursor| cursor.next);
        self.kept.iter().find(|kept| kept.number >= from)
    }

    /// Gives a follower never seen its place: the oldest alert of the last
    /// `FRESH`, or the next one to come.
    fn start(&mut self, id: EndpointId, now: Instant) {
        if self.cursors.contains_key(&id) {
            return;
        }

        let from = self
            .kept
            .iter()
            .find(|kept| now.saturating_duration_since(kept.at) <= FRESH)
            .map_or(self.next, |kept| kept.number);
        self.place(id, from);
    }

    /// Notes that `id` has had everything up to `number`.
    fn had(&mut self, id: EndpointId, number: u64) {
        self.place(id, number + 1);
    }

    fn place(&mut self, id: EndpointId, next: u64) {
        self.clock += 1;
        let cursor = Cursor {
            next,
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

impl<T: Clone> Follow<'_, T> {
    /// The next alert this follower has not had, once there is one. It is
    /// the same alert until `sent` says it went out.
    pub(crate) async fn next(&self) -> T {
        loop {
            // Made before looking, so an alert that comes between is not missed.
            let arrived = self.hub.arrived.notified();
            {
                let mut held = lock(&self.hub.held);
                held.start(self.id, Instant::now());

                if let Some(kept) = held.after(self.id) {
                    self.given.store(kept.number + 1, Ordering::Relaxed);
                    return kept.alert.clone();
                }
            }
            arrived.await;
        }
    }

    /// The alert `next` gave went out: the follower has had it.
    pub(crate) fn sent(&self) {
        let Some(number) = self.given.swap(0, Ordering::Relaxed).checked_sub(1) else {
            return;
        };

        lock(&self.hub.held).had(self.id, number);
    }
}

#[cfg(test)]
#[path = "../tests/agents_alerts.rs"]
mod tests;
