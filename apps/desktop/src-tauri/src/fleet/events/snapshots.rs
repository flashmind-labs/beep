//! Read projections owned by the currently demanded node streams.

use beep_mesh::{MeshError, SnapshotFuture, SnapshotReader};
use beep_proto::node::events::{self, Query, Read, Snapshot, Topic, Value};
use beep_proto::watched;
use iroh::EndpointId;
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};
use tokio::sync::watch;
use uuid::Uuid;

pub(crate) type Current = Option<std::result::Result<Value, String>>;

/// How long a live query keeps a changed value nobody read before it is
/// dropped and the node stops reading it. A view that follows it reads
/// again on each change; a read again within this reuses it.
pub(super) const LINGER: Duration = Duration::from_secs(30);

struct Entry {
    query: Query,
    stream: Topic,
    used: u64,
    current: watch::Sender<Current>,
    /// Since when it has held a value no reader took.
    unread: Option<Instant>,
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
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let machine = held
            .machines
            .get_mut(&id)
            .filter(|machine| machine.streams.values().any(|id| *id == generation))?;
        let Some(query) = snapshot.query else {
            return Some(false);
        };

        if let Some(entry) = machine.entries.get_mut(&query)
            && machine.streams.get(&entry.stream) == Some(&generation)
            && entry.query.topic == snapshot.topic
        {
            let initial = watched::read_sent(&entry.current, Option::is_none);
            entry.current.send_replace(Some(Ok(snapshot.value.clone())));

            // A change its followers are told of, and a waiting reader takes now.
            if !initial && entry.current.receiver_count() == 0 {
                entry.unread.get_or_insert_with(Instant::now);
            }
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

    /// Holds `read` on machine `id` for as long as the receiver lives: its node keeps pushing
    /// the value, and nothing sweeps it. `None` when the machine's stream for it is not open.
    pub(crate) fn follow(
        &self,
        id: EndpointId,
        read: Read,
    ) -> Option<std::result::Result<watch::Receiver<Current>, MeshError>> {
        self.register(id, read)
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
            .map(|(id, entry)| {
                (
                    *id,
                    watched::read_sent(&entry.current, |current| matches!(current, Some(Err(_)))),
                )
            })
        {
            if failed {
                machine.entries.remove(&existing);
            } else if let Some(entry) = machine.entries.get_mut(&existing) {
                entry.used = next;
                entry.unread = None;

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
                unread: None,
            },
        );
        drop(held);
        self.changed.send_replace(());
        Some(Ok(receiver))
    }
}

impl Snapshots {
    /// Drops the queries of `stream` on machine `id` that changed and were
    /// not read again within `LINGER` by `now`, so the node stops reading
    /// them; whether any went.
    pub(super) fn sweep(&self, id: EndpointId, stream: &Topic, now: Instant) -> bool {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(machine) = held.machines.get_mut(&id) else {
            return false;
        };
        let before = machine.entries.len();
        machine.entries.retain(|_, entry| {
            entry.stream != *stream
                || entry.current.receiver_count() > 0
                || entry
                    .unread
                    .is_none_or(|since| now.saturating_duration_since(since) < LINGER)
        });
        let swept = machine.entries.len() != before;
        drop(held);

        if swept {
            self.changed.send_replace(());
        }
        swept
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

/// How long a read waits for the first snapshot of its subscription before the caller reads
/// the node directly.
const SNAPSHOT_WAIT: Duration = Duration::from_secs(3);

/// The wait of a session listing: well under the 3 s it is allowed, so the direct read still
/// has time to run.
const LISTING_WAIT: Duration = Duration::from_secs(1);

fn wait_for_snapshot(read: &Read) -> Duration {
    match read.topic() {
        Some(topic) if topic.name() == "agents" => LISTING_WAIT,
        _ => SNAPSHOT_WAIT,
    }
}

struct Reader {
    snapshots: Arc<Snapshots>,
    id: EndpointId,
}

impl SnapshotReader for Reader {
    fn follow(&self, read: Read) -> Option<watch::Receiver<Current>> {
        self.snapshots.follow(self.id, read)?.ok()
    }

    fn snapshot(&self, read: Read) -> SnapshotFuture<'_> {
        Box::pin(async move {
            let wait = wait_for_snapshot(&read);
            let mut current = match self.snapshots.register(self.id, read)? {
                Ok(current) => current,
                Err(error) => return Some(Err(error)),
            };
            Some(
                match tokio::time::timeout(wait, watched::wait_for(&mut current, Option::is_some))
                    .await
                {
                    Ok(Ok(())) => match watched::read(&current, Clone::clone) {
                        Some(Ok(value)) => Ok(value),
                        Some(Err(reason)) => Err(error(&reason)),
                        None => Err(error("the subscription did not provide its snapshot")),
                    },
                    // The read can outlive its view's subscription.
                    Ok(Err(_)) => return None,
                    // A busy node can take longer. The caller asks it directly meanwhile, which is
                    // no failure, and the query stays registered: the snapshot that comes late
                    // serves the next read at once.
                    Err(_) => return None,
                },
            )
        })
    }
}

#[cfg(test)]
#[path = "../../tests/fleet_events_snapshots.rs"]
mod tests;
