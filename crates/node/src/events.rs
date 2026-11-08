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
}

impl Default for Hub {
    fn default() -> Self {
        Self(Arc::new(Inner {
            states: Mutex::new(BTreeMap::new()),
            changes: broadcast::channel(SIGNAL_CAPACITY).0,
        }))
    }
}

impl Hub {
    pub fn subscribe(&self) -> broadcast::Receiver<Change> {
        self.0.changes.subscribe()
    }

    pub fn changed(&self, topic: Topic) {
        if topic.valid() {
            let _ = self.0.changes.send(Change { topic });
        }
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
mod tests {
    use super::*;

    #[tokio::test]
    async fn plugin_reconnect_reads_latest_state_without_retaining_updates() {
        let hub = Hub::default();
        let topic = Topic::keyed("plugin", "example");
        let mut changes = hub.subscribe();
        hub.publish(topic.clone(), vec![1]).unwrap();
        hub.publish(topic.clone(), vec![2]).unwrap();
        assert_eq!(changes.recv().await.unwrap().topic, topic);
        assert_eq!(hub.current(&topic), Some(Value::Plugin(vec![2])));
        assert_eq!(crate::lock(&hub.0.states).len(), 1);
        assert!(hub.publish(Topic::new("plugin"), vec![]).is_err());
    }

    #[tokio::test]
    async fn unchanged_plugin_state_does_not_wake_readers() {
        let hub = Hub::default();
        let topic = Topic::keyed("plugin", "example");
        hub.publish(topic.clone(), vec![1]).unwrap();
        let mut changes = hub.subscribe();
        hub.publish(topic, vec![1]).unwrap();
        assert!(matches!(
            changes.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }
}
