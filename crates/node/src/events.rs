//! Bounded topic history, shared by node features and user plugins.

use beep_proto::node::events::{Event, EventId, Topic};
use std::{
    collections::VecDeque,
    sync::{Arc, LazyLock, Mutex},
};
use tokio::sync::broadcast;
use uuid::Uuid;

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

/// `None` means a snapshot is required; `Some` contains ordered events after the cursor.
pub struct Replay {
    pub cursor: EventId,
    pub events: Option<Vec<Event>>,
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
        let events = after
            .filter(|after| {
                after.epoch == history.cursor.epoch
                    && after.sequence <= history.cursor.sequence
                    && history
                        .entries
                        .front()
                        .is_none_or(|first| after.sequence >= first.id.sequence.saturating_sub(1))
            })
            .map(|after| {
                history
                    .entries
                    .iter()
                    .filter(|event| event.id.sequence > after.sequence)
                    .cloned()
                    .collect()
            });
        Replay {
            cursor: history.cursor,
            events,
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
        let initial = hub.cursor();
        hub.changed(Topic::new("files"));
        let first = hub.cursor();
        hub.changed(Topic::new("reviews"));
        let replay = hub.replay(Some(initial));
        assert_eq!(
            replay
                .events
                .unwrap()
                .iter()
                .map(|event| event.topic.name.as_str())
                .collect::<Vec<_>>(),
            ["files", "reviews"]
        );
        assert_eq!(hub.replay(Some(first)).events.unwrap().len(), 1);
        assert!(hub.replay(Some(hub.cursor())).events.unwrap().is_empty());
        assert!(Hub::default().replay(Some(first)).events.is_none());
        assert!(
            hub.replay(Some(EventId {
                sequence: 99,
                ..first
            }))
            .events
            .is_none()
        );
        for _ in 0..RETAINED {
            hub.changed(Topic::new("files"));
        }
        assert!(hub.replay(Some(first)).events.is_none());
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
        assert_eq!(replay.events.unwrap().len(), 2);
        hub.changed(Topic::new("after"));
        assert!(receiver.recv().await.unwrap().id.sequence <= replay.cursor.sequence);
        assert!(receiver.recv().await.unwrap().id.sequence > replay.cursor.sequence);
    }
}
