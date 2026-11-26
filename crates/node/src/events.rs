//! Change signals and the latest plugin state. Reconnect reads current resources.

use beep_proto::node::events::{Topic, Value};
use std::{
    collections::BTreeMap,
    panic::Location,
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
    pub kind: ChangeKind,
    /// Which paths under the topic's root changed, when the source knows.
    pub touched: Touched,
}

/// The most paths a change names; beyond it the paths are not known.
pub const MAX_TOUCHED: usize = 256;

/// The paths a change touched, relative to the root a `files` topic names. A change that does
/// not know them is `All`, and so is any that passed `MAX_TOUCHED`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Touched {
    /// Nothing yet: what a read has taken up so far.
    Nothing,
    Some(std::collections::BTreeSet<String>),
    #[default]
    All,
}

impl Touched {
    /// The paths of one change; `All` when there are none or too many.
    pub fn of(paths: impl IntoIterator<Item = String>) -> Self {
        let paths: std::collections::BTreeSet<_> = paths.into_iter().collect();

        if paths.is_empty() || paths.len() > MAX_TOUCHED {
            Self::All
        } else {
            Self::Some(paths)
        }
    }

    /// Takes `other` in: anything unknown makes the whole unknown.
    pub fn add(&mut self, other: Self) {
        *self = match (std::mem::replace(self, Self::Nothing), other) {
            (Self::All, _) | (_, Self::All) => Self::All,
            (Self::Nothing, other) | (other, Self::Nothing) => other,
            (Self::Some(mut held), Self::Some(more)) => {
                held.extend(more);
                Self::Some(held)
            }
        };

        if matches!(self, Self::Some(paths) if paths.len() > MAX_TOUCHED) {
            *self = Self::All;
        }
    }

    /// The paths as the wire says them: empty when they are not known.
    pub fn into_list(self) -> Vec<String> {
        match self {
            Self::Some(paths) => paths.into_iter().collect(),
            Self::Nothing | Self::All => Vec::new(),
        }
    }
}

/// How much of a topic's state a change can touch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChangeKind {
    /// Only files in a checkout's working tree; its Git refs and index stayed.
    WorkTree,
    /// Anything.
    #[default]
    Any,
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

    #[track_caller]
    pub fn changed(&self, topic: Topic) {
        self.changed_as(topic, ChangeKind::Any, Touched::All, Location::caller());
    }

    /// `changed`, or a change of files in the checkout only (`WorkTree`), saying which paths of the
    /// root changed.
    #[cfg(feature = "host")]
    #[track_caller]
    pub(crate) fn changed_touching(&self, topic: Topic, kind: ChangeKind, touched: Touched) {
        self.changed_as(topic, kind, touched, Location::caller());
    }

    /// Counts the change by its topic and by the line of code that sent it,
    /// which says what makes a topic change so often.
    #[cfg_attr(not(feature = "host"), allow(unused_variables))]
    fn changed_as(&self, topic: Topic, kind: ChangeKind, touched: Touched, from: &Location<'_>) {
        #[cfg(feature = "host")]
        if topic.valid() {
            let file = from.file();
            let file = file.rsplit_once("/src/").map_or(file, |(_, file)| file);
            metrics::counter!(
                "beep_topic_changed_total",
                "name" => topic.name().to_owned(),
                "from" => format!("{file}:{}", from.line()),
            )
            .increment(1);
        }

        self.forward(Change {
            topic,
            kind,
            touched,
        });
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

/// Woken when a session or terminal opens or closes.
#[cfg(feature = "host")]
static OPEN_CHANGED: tokio::sync::Notify = tokio::sync::Notify::const_new();

/// Says a session or terminal opened or closed.
#[cfg(feature = "host")]
pub(crate) fn open_changed() {
    OPEN_CHANGED.notify_one();
}

/// Waits for the next `open_changed`.
#[cfg(feature = "host")]
pub(crate) async fn opened_or_closed() {
    OPEN_CHANGED.notified().await;
}

/// Process and terminal registries are shared by embedded nodes too.
pub fn system() -> &'static Hub {
    static HUB: LazyLock<Hub> = LazyLock::new(Hub::default);
    &HUB
}

#[cfg(test)]
#[path = "tests/events.rs"]
mod tests;
