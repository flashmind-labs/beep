use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::instance::Instance;
use crate::lock::locked;
use crate::manifest::PluginId;
use crate::tool::{MAX_PENDING as MAX_CALLS, ToolCall};
use crate::ui::CallbackId;

/// The most events per second the host runs for one plugin.
pub(crate) const MAX_EVENTS_PER_SECOND: usize = 30;
/// The longest text an input event carries, in bytes.
pub const MAX_TEXT_BYTES: usize = 4096;
/// Events that wait for one plugin.
const MAX_PENDING: usize = 64;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EventError {
    #[error("text of {0} bytes is over the {MAX_TEXT_BYTES} byte limit")]
    TooLong(usize),
}

/// What the user did to an interactive node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Action {
    /// A button was pressed.
    Press,
    /// A toggle was switched to `value`.
    Toggle { value: bool },
    /// An input changed to `value`, or was submitted with it.
    Text { value: String },
}

impl Action {
    pub(crate) fn check(&self) -> Result<(), EventError> {
        match self {
            Action::Text { value } if value.len() > MAX_TEXT_BYTES => {
                Err(EventError::TooLong(value.len()))
            }
            _ => Ok(()),
        }
    }

    /// The argument the plugin's callback gets, as JSON.
    pub(crate) fn argument(&self) -> String {
        match self {
            Action::Press => "null".to_owned(),
            Action::Toggle { value } => value.to_string(),
            Action::Text { value } => serde_json::Value::from(value.as_str()).to_string(),
        }
    }
}

/// An event from the page for the callback `id` of `plugin`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiEvent {
    pub plugin: PluginId,
    pub id: CallbackId,
    pub action: Action,
}

/// Counts a plugin's events in the last second.
#[derive(Debug, Default)]
pub(crate) struct Limiter {
    recent: VecDeque<Instant>,
}

impl Limiter {
    /// Whether an event at `now` is within the limit. A refused event is not counted.
    pub(crate) fn allow(&mut self, now: Instant) -> bool {
        while self
            .recent
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) >= Duration::from_secs(1))
        {
            self.recent.pop_front();
        }
        if self.recent.len() >= MAX_EVENTS_PER_SECOND {
            return false;
        }

        self.recent.push_back(now);

        true
    }
}

/// Work for one plugin's thread besides topic updates: the page's instances,
/// events on its controls and tool calls.
#[derive(Default)]
pub(crate) struct Mailbox {
    instances: Mutex<Option<Arc<Vec<Instance>>>>,
    events: Mutex<Vec<UiEvent>>,
    calls: Mutex<Vec<ToolCall>>,
}

/// What a plugin's thread takes from its mailbox.
pub(crate) struct Mail {
    pub(crate) instances: Option<Arc<Vec<Instance>>>,
    pub(crate) events: Vec<UiEvent>,
    pub(crate) calls: Vec<ToolCall>,
}

impl Mailbox {
    pub(crate) fn set_instances(&self, list: Arc<Vec<Instance>>) {
        *locked(&self.instances) = Some(list);
    }

    /// Queues an event. A text change that follows one for the same control replaces
    /// it, since each carries the whole text. When the queue is full the event is dropped.
    pub(crate) fn push(&self, event: UiEvent) {
        let mut events = locked(&self.events);

        let replace = matches!(event.action, Action::Text { .. })
            && events.last().is_some_and(|last| {
                last.id == event.id && matches!(last.action, Action::Text { .. })
            });
        if replace {
            events.pop();
        }
        if events.len() >= MAX_PENDING {
            log::debug!("dropped an event for {}: too many waiting", event.plugin);
            return;
        }

        events.push(event);
    }

    /// Queues a tool call, or gives it back when too many wait.
    pub(crate) fn push_call(&self, call: ToolCall) -> Result<(), ToolCall> {
        let mut calls = locked(&self.calls);
        if calls.len() >= MAX_CALLS {
            return Err(call);
        }

        calls.push(call);

        Ok(())
    }

    pub(crate) fn take(&self) -> Mail {
        Mail {
            instances: locked(&self.instances).take(),
            events: std::mem::take(&mut *locked(&self.events)),
            calls: std::mem::take(&mut *locked(&self.calls)),
        }
    }
}

#[cfg(test)]
#[path = "tests/event.rs"]
mod tests;
