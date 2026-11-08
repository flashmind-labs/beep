//! Bounded topic history, shared by node features and user plugins.

use beep_proto::node::events::{Event, EventId, Topic};
use std::{
    collections::VecDeque,
    sync::{Arc, LazyLock, Mutex},
};
use tokio::sync::broadcast;
use uuid::Uuid;

pub(crate) mod exits;
pub(crate) mod paths;

const RETAINED: usize = 1024;

#[derive(Clone, Debug)]
pub struct Hub(Arc<Inner>);

#[derive(Debug)]
struct Inner {
    history: Mutex<History>,
    events: broadcast::Sender<Event>,
}

#[derive(Debug)]
struct History {
    cursor: EventId,
    entries: VecDeque<Event>,
}

/// Ordered events after the cursor, or an empty list when a snapshot is required.
pub struct Replay {
    pub cursor: EventId,
    pub events: Vec<Event>,
    pub resync: bool,
}

impl Default for Hub {
    fn default() -> Self {
        Self(Arc::new(Inner {
            history: Mutex::new(History {
                cursor: EventId {
                    epoch: Uuid::new_v4(),
                    sequence: 0,
                },
                entries: VecDeque::with_capacity(RETAINED),
            }),
            events: broadcast::channel(RETAINED).0,
        }))
    }
}

impl Hub {
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.0.events.subscribe()
    }

    pub fn cursor(&self) -> EventId {
        crate::lock(&self.0.history).cursor
    }

    /// Subscribe before calling this, then discard queued events at or before its cursor.
    pub fn replay(&self, after: Option<EventId>) -> Replay {
        let history = crate::lock(&self.0.history);
        let after = after.filter(|after| {
            after.epoch == history.cursor.epoch
                && after.sequence <= history.cursor.sequence
                && history
                    .entries
                    .front()
                    .is_none_or(|first| after.sequence >= first.id.sequence.saturating_sub(1))
        });
        let events = after
            .map(|after| {
                history
                    .entries
                    .iter()
                    .filter(|event| event.id.sequence > after.sequence)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        Replay {
            cursor: history.cursor,
            events,
            resync: after.is_none(),
        }
    }

    pub fn changed(&self, topic: Topic) {
        if !topic.valid() {
            return;
        }
        let mut history = crate::lock(&self.0.history);
        match history.cursor.sequence.checked_add(1) {
            Some(sequence) => history.cursor.sequence = sequence,
            None => {
                history.cursor = EventId {
                    epoch: Uuid::new_v4(),
                    sequence: 1,
                };
                history.entries.clear();
            }
        }
        let event = Event {
            topic,
            id: history.cursor,
        };
        if history.entries.len() == RETAINED {
            history.entries.pop_front();
        }
        history.entries.push_back(event.clone());
        let _ = self.0.events.send(event);
    }
}

/// Process and terminal registries are shared by embedded nodes too.
pub fn system() -> &'static Hub {
    static HUB: LazyLock<Hub> = LazyLock::new(Hub::default);
    &HUB
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_replays_a_disconnect_and_detects_expiry_restart_and_future_cursors() {
        let hub = Hub::default();
        let initial_snapshot = hub.replay(None);
        assert!(initial_snapshot.events.is_empty());
        assert!(initial_snapshot.resync);
        let initial = hub.cursor();
        hub.changed(Topic::new("files"));
        let first = hub.cursor();
        hub.changed(Topic::new("reviews"));
        let replay = hub.replay(Some(initial));
        assert_eq!(
            replay
                .events
                .iter()
                .map(|event| event.topic.name())
                .collect::<Vec<_>>(),
            ["files", "reviews"]
        );
        assert!(!replay.resync);
        assert_eq!(hub.replay(Some(first)).events.len(), 1);
        let caught_up = hub.replay(Some(hub.cursor()));
        assert!(caught_up.events.is_empty());
        assert!(!caught_up.resync);
        let restarted = Hub::default().replay(Some(first));
        assert!(restarted.events.is_empty());
        assert!(restarted.resync);
        let future = hub.replay(Some(EventId {
            sequence: 99,
            ..first
        }));
        assert!(future.events.is_empty());
        assert!(future.resync);
        for _ in 0..RETAINED {
            hub.changed(Topic::new("files"));
        }
        let expired = hub.replay(Some(first));
        assert!(expired.events.is_empty());
        assert!(expired.resync);
        assert_eq!(crate::lock(&hub.0.history).entries.len(), RETAINED);
    }

    #[tokio::test]
    async fn subscribe_and_replay_have_no_gap_or_duplicate() {
        let hub = Hub::default();
        let before = hub.cursor();
        hub.changed(Topic::new("first"));
        let mut receiver = hub.subscribe();
        hub.changed(Topic::new("during"));
        let replay = hub.replay(Some(before));
        assert_eq!(replay.events.len(), 2);
        assert!(!replay.resync);
        hub.changed(Topic::new("after"));
        assert!(receiver.recv().await.unwrap().id.sequence <= replay.cursor.sequence);
        assert!(receiver.recv().await.unwrap().id.sequence > replay.cursor.sequence);
    }
}
