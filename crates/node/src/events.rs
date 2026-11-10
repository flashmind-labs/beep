//! Change signals and the latest plugin state. Reconnect reads current resources.

use beep_proto::node::events::{Topic, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, LazyLock, Mutex},
};
use tokio::sync::broadcast;

host! {
    pub(crate) mod exits;
    pub(crate) mod paths;
}

const SIGNAL_CAPACITY: usize = 1024;
const MAX_PLUGIN_TOPICS: usize = 128;
const MAX_PLUGIN_STATE: usize = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Change {
    pub topic: Topic,
}

#[derive(Clone, Debug)]
pub struct Hub(Arc<Inner>);

#[derive(Debug)]
struct Inner {
    states: Mutex<BTreeMap<Topic, Value>>,
    changes: broadcast::Sender<Change>,
    topics: Mutex<BTreeMap<Topic, broadcast::WeakSender<Change>>>,
}

impl Default for Hub {
    fn default() -> Self {
        Self(Arc::new(Inner {
            states: Mutex::new(BTreeMap::new()),
            changes: broadcast::channel(SIGNAL_CAPACITY).0,
            topics: Mutex::default(),
        }))
    }
}

impl Hub {
    pub fn subscribe(&self) -> broadcast::Receiver<Change> {
        self.0.changes.subscribe()
    }

    pub fn changed(&self, topic: Topic) {
        #[cfg(feature = "host")]
        if topic.valid() {
            metrics::counter!("beep_topic_changed_total", "name" => topic.name().to_owned())
                .increment(1);
        }

        self.forward(Change { topic });
    }

    /// Passes on a change another hub counted already.
    pub(crate) fn forward(&self, change: Change) {
        if change.topic.valid() {
            let _ = self.0.changes.send(change.clone());
            crate::lock(&self.0.topics).retain(|topic, weak| {
                let Some(sender) = weak.upgrade() else {
                    return false;
                };

                if topic.matches(&change.topic) {
                    let _ = sender.send(change.clone());
                }

                true
            });
        }
    }

    #[cfg(feature = "host")]
    pub(crate) fn register(&self, topic: Topic, sender: &broadcast::Sender<Change>) {
        let mut topics = crate::lock(&self.0.topics);
        topics.retain(|_, sender| sender.strong_count() > 0);
        topics.insert(topic, sender.downgrade());
    }

    /// Replaces a plugin's current compact MessagePack state, never its event history.
    pub fn publish(&self, topic: Topic, state: Vec<u8>) -> anyhow::Result<()> {
        anyhow::ensure!(
            topic.valid() && !topic.is_pattern(),
            "a published topic must be exact"
        );
        anyhow::ensure!(state.len() <= MAX_PLUGIN_STATE, "plugin state is too large");
        {
            let mut states = crate::lock(&self.0.states);
            anyhow::ensure!(
                states.contains_key(&topic) || states.len() < MAX_PLUGIN_TOPICS,
                "too many plugin topics"
            );
            let value = Value::Plugin(state);

            if states.get(&topic) == Some(&value) {
                return Ok(());
            }

            states.insert(topic.clone(), value);
        }
        self.changed(topic);
        Ok(())
    }

    #[cfg(feature = "host")]
    pub(crate) fn current(&self, topic: &Topic) -> Option<Value> {
        let local = crate::lock(&self.0.states).get(topic).cloned();
        local.or_else(|| {
            (!Arc::ptr_eq(&self.0, &system().0))
                .then(|| crate::lock(&system().0.states).get(topic).cloned())
                .flatten()
        })
    }

    #[cfg(feature = "host")]
    pub(crate) fn published(&self) -> Vec<Topic> {
        let mut topics: std::collections::BTreeSet<_> =
            crate::lock(&self.0.states).keys().cloned().collect();
        if !Arc::ptr_eq(&self.0, &system().0) {
            topics.extend(crate::lock(&system().0.states).keys().cloned());
        }

        topics.into_iter().collect()
    }
}

/// Process and terminal registries are shared by embedded nodes too.
pub fn system() -> &'static Hub {
    static HUB: LazyLock<Hub> = LazyLock::new(Hub::default);
    &HUB
}

#[cfg(test)]
#[path = "tests/events.rs"]
mod tests;
