use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ident::Slot;

/// Instances of one slot the host keeps. The page reports no more.
pub const MAX_INSTANCES_PER_SLOT: usize = 200;
/// Instances and gone marks in one batch.
const MAX_BATCH: usize = 1000;
const MAX_KEY_LEN: usize = 128;
/// The longest text in a context, in bytes.
const MAX_FIELD_LEN: usize = 512;
/// The largest context, as JSON, a plugin is given.
const MAX_CONTEXT_BYTES: usize = 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum InstanceError {
    #[error("invalid instance key {0:?}: use 1 to {MAX_KEY_LEN} bytes without control characters")]
    Key(String),
    #[error("context text of {0} bytes is over the {MAX_FIELD_LEN} byte limit")]
    Field(usize),
    #[error("context of {0} bytes is over the {MAX_CONTEXT_BYTES} byte limit")]
    Context(usize),
    #[error("batch of {0} changes is over the limit of {MAX_BATCH}")]
    Batch(usize),
}

/// Tells the instances of one slot apart. The page picks it, such as a session's id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct InstanceKey(String);

impl TryFrom<String> for InstanceKey {
    type Error = InstanceError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        if text.is_empty() || text.len() > MAX_KEY_LEN || text.chars().any(char::is_control) {
            return Err(InstanceError::Key(text));
        }

        Ok(Self(text))
    }
}

impl From<InstanceKey> for String {
    fn from(key: InstanceKey) -> Self {
        key.0
    }
}

impl fmt::Display for InstanceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The message box of a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComposerContext {
    pub machine: String,
    pub session: String,
}

/// A session's row in the session list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionRowContext {
    pub machine: String,
    pub session: String,
    /// The title the row shows.
    pub title: String,
}

/// The header of the open session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionHeaderContext {
    pub machine: String,
    pub session: String,
}

/// A slot and what the page says about one instance of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "slot", content = "context")]
pub enum SlotContext {
    #[serde(rename = "composer")]
    Composer(ComposerContext),
    #[serde(rename = "session.row")]
    SessionRow(SessionRowContext),
    #[serde(rename = "session.header")]
    SessionHeader(SessionHeaderContext),
}

impl SlotContext {
    pub fn slot(&self) -> Slot {
        match self {
            SlotContext::Composer(_) => Slot::Composer,
            SlotContext::SessionRow(_) => Slot::SessionRow,
            SlotContext::SessionHeader(_) => Slot::SessionHeader,
        }
    }

    fn texts(&self) -> Vec<&str> {
        match self {
            SlotContext::Composer(c) => vec![&c.machine, &c.session],
            SlotContext::SessionRow(c) => vec![&c.machine, &c.session, &c.title],
            SlotContext::SessionHeader(c) => vec![&c.machine, &c.session],
        }
    }

    fn check(&self) -> Result<(), InstanceError> {
        if let Some(text) = self.texts().into_iter().find(|t| t.len() > MAX_FIELD_LEN) {
            return Err(InstanceError::Field(text.len()));
        }

        let size = match self {
            SlotContext::Composer(c) => serde_json::to_string(c),
            SlotContext::SessionRow(c) => serde_json::to_string(c),
            SlotContext::SessionHeader(c) => serde_json::to_string(c),
        }
        .map_or(0, |json| json.len());
        if size > MAX_CONTEXT_BYTES {
            return Err(InstanceError::Context(size));
        }

        Ok(())
    }
}

/// One place a slot is on the page now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instance {
    pub key: InstanceKey,
    #[serde(flatten)]
    pub context: SlotContext,
}

/// An instance that left the page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceRef {
    pub slot: Slot,
    pub key: InstanceKey,
}

/// What changed on the page since the last batch. `reset` says the page
/// started over, so everything the host held is gone first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceBatch {
    #[serde(default)]
    pub reset: bool,
    #[serde(default)]
    pub shown: Vec<Instance>,
    #[serde(default)]
    pub gone: Vec<InstanceRef>,
}

impl InstanceBatch {
    pub(crate) fn check(&self) -> Result<(), InstanceError> {
        let size = self.shown.len() + self.gone.len();
        if size > MAX_BATCH {
            return Err(InstanceError::Batch(size));
        }

        self.shown
            .iter()
            .try_for_each(|instance| instance.context.check())
    }
}

/// The instances on the page now, as the batches built them.
#[derive(Debug, Default)]
pub(crate) struct Instances {
    live: BTreeMap<(Slot, InstanceKey), SlotContext>,
}

impl Instances {
    /// Applies a checked batch. Returns whether the set changed. An instance
    /// of a slot that already holds the most is dropped, unless it replaces one.
    pub(crate) fn apply(&mut self, batch: InstanceBatch) -> bool {
        let before = self.live.clone();

        if batch.reset {
            self.live.clear();
        }
        for gone in batch.gone {
            self.live.remove(&(gone.slot, gone.key));
        }
        for instance in batch.shown {
            let id = (instance.context.slot(), instance.key);
            let held = self.live.keys().filter(|(slot, _)| *slot == id.0).count();
            if held >= MAX_INSTANCES_PER_SLOT && !self.live.contains_key(&id) {
                log::warn!("too many {} instances, dropped one", id.0);
                continue;
            }
            self.live.insert(id, instance.context);
        }

        self.live != before
    }

    /// Whether the page shows any instance of `slot`.
    pub(crate) fn has(&self, slot: Slot) -> bool {
        self.live.keys().any(|(held, _)| *held == slot)
    }

    pub(crate) fn list(&self) -> Arc<Vec<Instance>> {
        let list = self
            .live
            .iter()
            .map(|((_, key), context)| Instance {
                key: key.clone(),
                context: context.clone(),
            })
            .collect();

        Arc::new(list)
    }
}

#[cfg(test)]
#[path = "tests/instance.rs"]
mod tests;
