//! Event feed of a running run. Events are kept in memory only while the run is
//! active; the finished turn is saved with the conversation.

use crate::protocol::{Event, RunEvent};
use flashmind::core::CancellationToken;
use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard, PoisonError},
};
use tokio::sync::broadcast;

pub struct LiveRun {
    pub cancel: CancellationToken,
    /// Fires once the run is saved and gone from the live runs.
    pub done: CancellationToken,
    events: Mutex<Vec<Event>>,
    tx: broadcast::Sender<Event>,
}

/// Events a subscriber missed, plus a receiver for the ones that follow.
pub struct Subscription {
    pub backlog: Vec<Event>,
    pub live: broadcast::Receiver<Event>,
}

impl LiveRun {
    pub fn new(cancel: CancellationToken) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            cancel,
            done: CancellationToken::new(),
            events: Mutex::new(Vec::new()),
            tx,
        }
    }

    /// Numbers the event and sends it to every subscriber.
    pub fn push(&self, event: RunEvent) {
        let mut events = self.events();
        let event = Event {
            seq: events.len() as i64 + 1,
            event,
        };
        // No subscribers is fine, the event stays in the backlog.
        let _ = self.tx.send(event.clone());
        events.push(event);
    }

    pub fn subscribe(&self, after: i64) -> Subscription {
        // Hold the lock so no event lands between the backlog and the receiver.
        let events = self.events();
        Subscription {
            backlog: Self::after(&events, after),
            live: self.tx.subscribe(),
        }
    }

    /// Events after `after`, for a subscriber that fell behind the channel.
    pub fn since(&self, after: i64) -> Vec<Event> {
        Self::after(&self.events(), after)
    }

    /// What each tool call of the run showed when it started, by call id.
    pub fn tool_descriptions(&self) -> HashMap<String, String> {
        self.events()
            .iter()
            .filter_map(|event| match &event.event {
                RunEvent::ToolStarted {
                    call_id,
                    description,
                    ..
                } => Some((call_id.clone(), description.clone())),
                _ => None,
            })
            .collect()
    }

    fn after(events: &[Event], after: i64) -> Vec<Event> {
        let start = usize::try_from(after).unwrap_or(0).min(events.len());
        events[start..].to_vec()
    }

    fn events(&self) -> MutexGuard<'_, Vec<Event>> {
        self.events.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
