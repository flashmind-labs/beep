use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::ident::Permission;
use crate::lock::locked;

/// Subscriptions one plugin may hold.
pub(crate) const MAX_SUBSCRIPTIONS: usize = 16;
/// The largest event, as JSON, a plugin is given.
pub(crate) const MAX_EVENT_BYTES: usize = 256 * 1024;
/// Updates that wait for one plugin.
const MAX_PENDING: usize = 256;
const MAX_NAME_LEN: usize = 128;
const MAX_KEY_LEN: usize = 1024;
/// Names with this prefix are local to the app and never go to the mesh.
const LOCAL_PREFIX: &str = "ui.";

/// A topic of the app's own, as the page follows it: a name, an optional key
/// (none means every key) and an optional machine (none means any).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Topic {
    pub name: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub machine: Option<String>,
}

/// What a topic's name says about who may follow it.
enum Family {
    /// `ui.*`: state the page publishes itself.
    Local,
    /// Topics that expose sessions: `session.turn`, which the page builds, and the mesh's.
    Sessions,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TopicError {
    #[error("invalid topic: {0}")]
    Invalid(String),
    #[error("unknown topic {0:?}: plugins can follow ui.* topics, session.turn, agents and agent")]
    Unknown(String),
    #[error("topic {topic:?} needs the {permission} permission")]
    Denied {
        topic: String,
        permission: Permission,
    },
    #[error("at most {MAX_SUBSCRIPTIONS} subscriptions")]
    TooMany,
    #[error("event is {0} bytes, the limit is {MAX_EVENT_BYTES}")]
    TooLarge(usize),
}

impl Topic {
    /// A topic of the app's own, followed by every key and machine.
    pub fn local(name: &str) -> Result<Self, TopicError> {
        let topic = Self {
            name: name.to_owned(),
            key: None,
            machine: None,
        };
        topic.check()?;
        if !topic.is_local() {
            return Err(TopicError::Unknown(name.to_owned()));
        }

        Ok(topic)
    }

    /// Whether the page publishes this topic itself.
    pub fn is_local(&self) -> bool {
        self.name.starts_with(LOCAL_PREFIX)
    }

    /// The permission following this topic needs, if any. Refuses names no plugin may follow.
    fn permission(&self) -> Result<Option<Permission>, TopicError> {
        self.check()?;

        Ok(match self.family()? {
            Family::Local => None,
            Family::Sessions => Some(Permission::SessionsRead),
        })
    }

    /// Whether the topic is one the mesh pushes: the sessions' `agents` and `agent`.
    pub fn on_mesh(&self) -> bool {
        matches!(self.name.as_str(), "agents" | "agent")
    }

    fn family(&self) -> Result<Family, TopicError> {
        match self.name.as_str() {
            "session.turn" => Ok(Family::Sessions),
            _ if self.on_mesh() => Ok(Family::Sessions),
            _ if self.is_local() => Ok(Family::Local),
            other => Err(TopicError::Unknown(other.to_owned())),
        }
    }

    /// Checks the shape of every part, whatever the name means.
    fn check(&self) -> Result<(), TopicError> {
        let word = |text: &str| {
            text.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        };
        let machine = self.machine.as_deref().unwrap_or("x");

        if self.name.is_empty() || self.name.len() > MAX_NAME_LEN || !word(&self.name) {
            return Err(TopicError::Invalid(format!("name {:?}", self.name)));
        }
        if self
            .key
            .as_deref()
            .is_some_and(|key| key.is_empty() || key.len() > MAX_KEY_LEN || key.contains('\0'))
        {
            return Err(TopicError::Invalid("key".to_owned()));
        }
        if machine.is_empty() || machine.len() > MAX_NAME_LEN || !word(machine) {
            return Err(TopicError::Invalid("machine".to_owned()));
        }

        Ok(())
    }
}

/// An update to a topic, as the page sends it to the host. `topic` is the one
/// the page subscribed on a plugin's behalf; `machine` is where the update came
/// from, none for `ui.*` topics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Publication {
    pub topic: Topic,
    #[serde(default)]
    pub machine: Option<String>,
    pub data: Value,
}

/// A checked update, serialized once for every plugin that gets it.
#[derive(Debug, Clone)]
pub(crate) struct Delivery {
    pub(crate) topic: Topic,
    pub(crate) machine: Option<String>,
    json: Arc<str>,
}

impl Delivery {
    pub(crate) fn new(publication: &Publication) -> Result<Self, TopicError> {
        publication.topic.permission()?;

        let json = serde_json::to_string(publication)
            .map_err(|error| TopicError::Invalid(error.to_string()))?;
        if json.len() > MAX_EVENT_BYTES {
            return Err(TopicError::TooLarge(json.len()));
        }

        Ok(Self {
            topic: publication.topic.clone(),
            machine: publication.machine.clone(),
            json: json.into(),
        })
    }

    /// The event as the plugin's callback gets it: `{ topic, machine, data }`.
    pub(crate) fn json(&self) -> &str {
        &self.json
    }

    fn same_stream(&self, other: &Self) -> bool {
        self.topic == other.topic && self.machine == other.machine
    }
}

/// The topics one plugin follows, checked against its permissions.
pub(crate) struct Subscriptions {
    permissions: Vec<Permission>,
    list: Vec<(u32, Topic)>,
    changed: bool,
}

impl Subscriptions {
    pub(crate) fn new(permissions: &[Permission]) -> Self {
        Self {
            permissions: permissions.to_vec(),
            list: Vec::new(),
            // The first report says what a plugin follows, even nothing.
            changed: true,
        }
    }

    /// Adds the subscription `id` for the topic in `json`.
    pub(crate) fn add(&mut self, id: u32, json: &str) -> Result<(), TopicError> {
        let topic: Topic =
            serde_json::from_str(json).map_err(|error| TopicError::Invalid(error.to_string()))?;

        if let Some(permission) = topic.permission()?
            && !self.permissions.contains(&permission)
        {
            return Err(TopicError::Denied {
                topic: topic.name,
                permission,
            });
        }
        if self.list.len() >= MAX_SUBSCRIPTIONS {
            return Err(TopicError::TooMany);
        }

        self.list.push((id, topic));
        self.changed = true;

        Ok(())
    }

    pub(crate) fn remove(&mut self, id: u32) {
        let before = self.list.len();
        self.list.retain(|(held, _)| *held != id);
        self.changed |= self.list.len() != before;
    }

    /// The distinct topics followed, once after each change.
    pub(crate) fn take_changed(&mut self) -> Option<Vec<Topic>> {
        if !self.changed {
            return None;
        }
        self.changed = false;

        let mut topics: Vec<Topic> = self.list.iter().map(|(_, topic)| topic.clone()).collect();
        topics.sort();
        topics.dedup();

        Some(topics)
    }
}

/// Updates waiting for one plugin's thread. Only the newest of each stream waits.
#[derive(Default)]
pub(crate) struct Inbox {
    pending: std::sync::Mutex<Vec<Delivery>>,
}

impl Inbox {
    pub(crate) fn push(&self, delivery: Delivery) {
        let mut pending = locked(&self.pending);

        if let Some(slot) = pending.iter_mut().find(|held| held.same_stream(&delivery)) {
            *slot = delivery;
        } else {
            if pending.len() >= MAX_PENDING {
                pending.remove(0);
            }
            pending.push(delivery);
        }
    }

    pub(crate) fn take(&self) -> Vec<Delivery> {
        std::mem::take(&mut *locked(&self.pending))
    }
}

#[cfg(test)]
#[path = "tests/topic.rs"]
mod tests;
