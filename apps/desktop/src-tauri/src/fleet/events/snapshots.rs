//! Read projections owned by the currently demanded node streams.

use beep_mesh::{MeshError, SnapshotFuture, SnapshotReader};
use beep_proto::node::events::{self, Query, Read, Snapshot, Topic, Value};
use iroh::EndpointId;
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tokio::sync::watch;
use uuid::Uuid;

type Current = Option<std::result::Result<Value, String>>;

struct Entry {
    query: Query,
    stream: Topic,
    used: u64,
    current: watch::Sender<Current>,
}

struct Machine {
    topics: Vec<Topic>,
    streams: BTreeMap<Topic, Uuid>,
    entries: BTreeMap<u64, Entry>,
}

#[derive(Default)]
struct Held {
    next: u64,
    machines: HashMap<EndpointId, Machine>,
}

pub(crate) struct Snapshots {
    held: Mutex<Held>,
    changed: watch::Sender<()>,
}

impl Default for Snapshots {
    fn default() -> Self {
        Self {
            held: Mutex::default(),
            changed: watch::channel(()).0,
        }
    }
}

impl Snapshots {
    pub(super) fn reconcile(&self, wanted: &BTreeMap<EndpointId, Vec<Topic>>) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let mut changed = held.machines.len() != wanted.len();
        held.machines.retain(|id, _| wanted.contains_key(id));

        for (id, topics) in wanted {
            let machine = held.machines.entry(*id).or_insert_with(|| Machine {
                topics: Vec::new(),
                streams: BTreeMap::new(),
                entries: BTreeMap::new(),
            });

            if machine.topics != *topics {
                changed = true;
                machine.topics.clone_from(topics);
            }

            machine.streams.retain(|topic, _| topics.contains(topic));

            let before = machine.entries.len();
            machine
                .entries
                .retain(|_, entry| topics.contains(&entry.stream));
            changed |= before != machine.entries.len();
        }

        drop(held);

        if changed {
            self.changed.send_replace(());
        }
    }

    pub(super) fn connected(&self, id: EndpointId, topic: Topic, generation: Uuid) -> bool {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);

        if let Some(machine) = held.machines.get_mut(&id)
            && machine.topics.contains(&topic)
        {
            machine.streams.insert(topic.clone(), generation);

            for entry in machine.entries.values() {
                if topic == entry.stream {
                    entry.current.send_replace(None);
                }
            }
            return true;
        }

        false
    }

    pub(super) fn end(&self, id: EndpointId, generation: Uuid) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);

        if let Some(machine) = held.machines.get_mut(&id)
            && let Some(topic) = machine
                .streams
                .iter()
                .find(|(_, id)| **id == generation)
                .map(|(topic, _)| topic.clone())
        {
            machine.streams.remove(&topic);
            // Only readers of this stream fall back to finite requests.
            machine.entries.retain(|_, entry| topic != entry.stream);
        }
    }

    pub(super) fn queries_for(&self, id: EndpointId, topic: &Topic) -> Vec<Query> {
        let held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        held.machines
            .get(&id)
            .map(|machine| {
                machine
                    .entries
                    .values()
                    .filter(|entry| *topic == entry.stream)
                    .map(|entry| entry.query.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn queries(&self, id: EndpointId) -> Vec<Query> {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .machines
            .get(&id)
            .map(|machine| {
                machine
                    .entries
                    .values()
                    .map(|entry| entry.query.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn changes(&self) -> watch::Receiver<()> {
        self.changed.subscribe()
    }

    pub(super) fn install(
        &self,
        id: EndpointId,
        generation: Uuid,
        snapshot: &Snapshot,
    ) -> Option<bool> {
        let held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let machine = held
            .machines
            .get(&id)
            .filter(|machine| machine.streams.values().any(|id| *id == generation))?;
        let Some(query) = snapshot.query else {
            return Some(false);
        };

        if let Some(entry) = machine.entries.get(&query)
            && machine.streams.get(&entry.stream) == Some(&generation)
            && entry.query.topic == snapshot.topic
        {
            let initial = entry.current.borrow().is_none();
            entry.current.send_replace(Some(Ok(snapshot.value.clone())));
            return Some(initial);
        }

        None
    }

    pub(super) fn failed(
        &self,
        id: EndpointId,
        generation: Uuid,
        query: u64,
        reason: String,
    ) -> bool {
        let held = self.held.lock().unwrap_or_else(PoisonError::into_inner);

        if let Some(machine) = held.machines.get(&id)
            && let Some(entry) = machine.entries.get(&query)
            && machine.streams.get(&entry.stream) == Some(&generation)
        {
            entry.current.send_replace(Some(Err(reason)));
            return true;
        }

        false
    }

    pub(crate) fn reader(self: &Arc<Self>, id: EndpointId) -> Arc<dyn SnapshotReader> {
        Arc::new(Reader {
            snapshots: self.clone(),
            id,
        })
    }

    fn register(
        &self,
        id: EndpointId,
        read: Read,
    ) -> Option<std::result::Result<watch::Receiver<Current>, MeshError>> {
        let topic = read.topic()?;
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let machine = held.machines.get(&id)?;

        let stream = machine
            .topics
            .iter()
            .find(|wanted| demanded(&topic, std::slice::from_ref(wanted)))?
            .clone();
        if !machine.streams.contains_key(&stream) {
            return None;
        }

        let next = match held.next.checked_add(1) {
            Some(next) => next,
            None => return Some(Err(error("subscription query sequence exhausted"))),
        };
        held.next = next;
        let machine = held.machines.get_mut(&id)?;

        if let Some((existing, failed)) = machine
            .entries
            .iter()
            .find(|(_, entry)| entry.query.read == read)
            .map(|(id, entry)| (*id, matches!(entry.current.borrow().as_ref(), Some(Err(_)))))
        {
            if failed {
                machine.entries.remove(&existing);
            } else if let Some(entry) = machine.entries.get_mut(&existing) {
                entry.used = next;

                return Some(Ok(entry.current.subscribe()));
            }
        }

        if machine.entries.len() >= events::MAX_QUERIES
            && let Some(oldest) = machine
                .entries
                .iter()
                .filter(|(_, entry)| entry.current.receiver_count() == 0)
                .min_by_key(|(_, entry)| entry.used)
                .map(|(id, _)| *id)
        {
            machine.entries.remove(&oldest);
        }

        if machine.entries.len() >= events::MAX_QUERIES {
            return Some(Err(error("too many snapshot reads are waiting")));
        }

        let (current, receiver) = watch::channel(None);
        machine.entries.insert(
            next,
            Entry {
                query: Query {
                    id: next,
                    topic,
                    read,
                },
                used: next,
                stream,
                current,
            },
        );
        drop(held);
        self.changed.send_replace(());
        Some(Ok(receiver))
    }
}

fn demanded(source: &Topic, wanted: &[Topic]) -> bool {
    wanted.iter().any(|wanted| {
        wanted.matches(source)
            || matches!(
                (source.name(), wanted.name()),
                ("agents", "agent" | "moves")
                    | ("agent", "agents")
                    | ("processes", "process-output")
                    | ("terminals", "terminal-output")
            )
            || (source.name() == "files"
                && wanted.name() == "files"
                && (wanted.resource_segments().len() >= 2
                    || wanted.is_pattern() && !wanted.resource_segments().is_empty()))
    })
}

fn error(message: &str) -> MeshError {
    MeshError::Link(anyhow::anyhow!(message.to_owned()))
}

struct Reader {
    snapshots: Arc<Snapshots>,
    id: EndpointId,
}

impl SnapshotReader for Reader {
    fn snapshot(&self, read: Read) -> SnapshotFuture<'_> {
        Box::pin(async move {
            let mut current = match self.snapshots.register(self.id, read)? {
                Ok(current) => current,
                Err(error) => return Some(Err(error)),
            };
            Some(
                match tokio::time::timeout(
                    Duration::from_secs(30),
                    current.wait_for(Option::is_some),
                )
                .await
                {
                    Ok(Ok(value)) => match value.as_ref() {
                        Some(Ok(value)) => Ok(value.clone()),
                        Some(Err(reason)) => Err(error(reason)),
                        None => Err(error("the subscription did not provide its snapshot")),
                    },
                    // The read can outlive its view's subscription.
                    Ok(Err(_)) => return None,
                    Err(_) => Err(error("the node did not provide its subscription snapshot")),
                },
            )
        })
    }
}

#[cfg(test)]
#[path = "../../tests/fleet_events_snapshots.rs"]
mod tests;
