//! Read projections owned by the currently demanded node streams: what a node pushes, kept
//! for the clients that read it.

use crate::MeshError;
use beep_proto::node::events::{
    self, MOVE_PROGRESS_TOPIC, Query, Read, Snapshot, TEAM_CARDS_TOPIC, TEAM_INVITES_TOPIC, Topic,
    Value,
};
use beep_proto::watched;
use iroh::EndpointId;
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};
use tokio::sync::{broadcast, watch};
use uuid::Uuid;

/// A subscribed read as it stands: `None` until its first value, then each value the node
/// pushes, or the reason it gave for failing.
pub type Followed = Option<std::result::Result<Value, String>>;

/// Demand for a subscribed read that lasts until it is dropped.
pub type Hold = Box<dyn Send + Sync>;

/// How long a live query keeps a changed value nobody read before it is
/// dropped and the node stops reading it. A view that follows it reads
/// again on each change; a read again within this reuses it.
pub const LINGER: Duration = Duration::from_secs(30);

struct Entry {
    query: Query,
    stream: Topic,
    used: u64,
    current: watch::Sender<Followed>,
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

/// What a node said on a topic, told to whoever follows `Snapshots::heard`.
#[derive(Clone)]
pub struct Heard {
    pub machine: EndpointId,
    pub topic: Topic,
    /// What it answered; `None` when it refused the topic.
    answer: Option<Arc<Snapshot>>,
}

impl Heard {
    pub fn said(machine: EndpointId, snapshot: &Arc<Snapshot>) -> Self {
        Self {
            machine,
            topic: snapshot.topic.clone(),
            answer: Some(snapshot.clone()),
        }
    }

    pub fn refused(machine: EndpointId, topic: Topic) -> Self {
        Self {
            machine,
            topic,
            answer: None,
        }
    }

    pub fn value(&self) -> Option<&Value> {
        self.answer.as_ref().map(|snapshot| &snapshot.value)
    }
}

/// What the nodes pushed on the streams a desktop holds open, and the demand that keeps them open.
pub struct Snapshots {
    held: Mutex<Held>,
    /// What the nodes say, for the bell and the team pages to follow.
    heard: broadcast::Sender<Heard>,
    /// The last thing said on each followed topic, for a follower that missed some to read again.
    latest: Mutex<HashMap<(EndpointId, Topic), Heard>>,
    changed: watch::Sender<()>,
    /// The topics waits on a node hold open, whatever views are open, one entry per hold.
    holds: watch::Sender<Vec<(EndpointId, Topic)>>,
}

/// A wait's demand for a topic on one machine, given up when it is dropped.
struct Holding {
    snapshots: Arc<Snapshots>,
    machine: EndpointId,
    topic: Topic,
}

impl Drop for Holding {
    fn drop(&mut self) {
        self.snapshots.holds.send_modify(|holds| {
            if let Some(at) = holds
                .iter()
                .position(|(machine, topic)| *machine == self.machine && *topic == self.topic)
            {
                holds.remove(at);
            }
        });
    }
}

impl Default for Snapshots {
    fn default() -> Self {
        Self {
            held: Mutex::default(),
            heard: broadcast::channel(HEARD_BACKLOG).0,
            latest: Mutex::default(),
            changed: watch::channel(()).0,
            holds: watch::channel(Vec::new()).0,
        }
    }
}

/// How many nodes' answers wait for a follower before it is told it missed some.
const HEARD_BACKLOG: usize = 1024;

impl Snapshots {
    /// Tells the followers what a node said.
    pub fn tell(&self, heard: Heard) {
        // Only what the bell and the team pages follow goes through: a busy topic such as a
        // terminal screen never fills the backlog.
        if !followed(&heard.topic) {
            return;
        }

        self.latest
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert((heard.machine, heard.topic.clone()), heard.clone());
        // No follower is no news to anyone.
        let _ = self.heard.send(heard);
    }

    /// What each machine last said on each followed topic: a follower that was told it missed
    /// some takes its state from here.
    pub fn latest(&self) -> Vec<Heard> {
        self.latest
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .cloned()
            .collect()
    }

    pub fn heard(&self) -> broadcast::Receiver<Heard> {
        self.heard.subscribe()
    }

    /// Demands what waits hold on top of what views want, for the machines in `online`.
    pub fn add_holds(&self, wanted: &mut BTreeMap<EndpointId, Vec<Topic>>, online: &[EndpointId]) {
        let holds = watched::read_sent(&self.holds, Clone::clone);

        for (machine, topic) in holds {
            if online.contains(&machine) {
                let topics = wanted.entry(machine).or_default();
                topics.push(topic);
                topics.sort();
                topics.dedup();
            }
        }
    }

    pub fn holds(&self) -> watch::Receiver<Vec<(EndpointId, Topic)>> {
        self.holds.subscribe()
    }

    pub fn reconcile(&self, wanted: &BTreeMap<EndpointId, Vec<Topic>>) {
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

        let kept: Vec<(EndpointId, Vec<Topic>)> = held
            .machines
            .iter()
            .map(|(id, machine)| (*id, machine.topics.clone()))
            .collect();
        drop(held);

        self.latest
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|(id, topic), _| {
                kept.iter()
                    .any(|(kept, topics)| kept == id && demanded(topic, topics))
            });

        if changed {
            self.changed.send_replace(());
        }
    }

    pub fn connected(&self, id: EndpointId, topic: Topic, generation: Uuid) -> bool {
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
            drop(held);
            // Waits for the stream to open follow this.
            self.changed.send_replace(());
            return true;
        }

        false
    }

    pub fn end(&self, id: EndpointId, generation: Uuid) {
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

    pub fn queries_for(&self, id: EndpointId, topic: &Topic) -> Vec<Query> {
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

    pub fn queries(&self, id: EndpointId) -> Vec<Query> {
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

    pub fn changes(&self) -> watch::Receiver<()> {
        self.changed.subscribe()
    }

    pub fn install(&self, id: EndpointId, generation: Uuid, snapshot: &Snapshot) -> Option<bool> {
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

    pub fn failed(&self, id: EndpointId, generation: Uuid, query: u64, reason: String) -> bool {
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

    /// Holds `read` on machine `id` for as long as the receiver lives: its node keeps pushing
    /// the value, and nothing sweeps it. `None` when the machine's stream for it is not open.
    pub fn register(
        &self,
        id: EndpointId,
        read: Read,
    ) -> Option<std::result::Result<watch::Receiver<Followed>, MeshError>> {
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
    pub fn sweep(&self, id: EndpointId, stream: &Topic, now: Instant) -> bool {
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

/// Whether the bell or the team pages follow what a node says on `topic`.
fn followed(topic: &Topic) -> bool {
    matches!(
        topic.name(),
        "sync" | "moves" | MOVE_PROGRESS_TOPIC | TEAM_CARDS_TOPIC | TEAM_INVITES_TOPIC
    )
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

impl Snapshots {
    /// Keeps `read` subscribed on machine `id`, whatever views are open, until the hold is
    /// dropped.
    pub fn hold(self: &Arc<Self>, id: EndpointId, read: &Read) -> Hold {
        let Some(topic) = read.topic() else {
            return Box::new(());
        };
        self.holds
            .send_modify(|holds| holds.push((id, topic.clone())));

        Box::new(Holding {
            snapshots: self.clone(),
            machine: id,
            topic,
        })
    }

    /// `read` on machine `id` as its subscription holds it, changing with each push, once the
    /// subscription is open; `None` when none can own it. A subscription that ends or fails is
    /// followed again by calling this again.
    pub async fn subscribe(&self, id: EndpointId, read: Read) -> Option<watch::Receiver<Followed>> {
        read.topic()?;
        let mut changes = self.changes();

        loop {
            match self.register(id, read.clone()) {
                Some(Ok(current)) => return Some(current),
                Some(Err(_)) => return None,
                None => {}
            }

            changes.changed().await.ok()?;
        }
    }

    /// The current value of `read` on machine `id` from its subscription, or `None` to use an
    /// ordinary request.
    pub async fn snapshot(&self, id: EndpointId, read: Read) -> Option<Result<Value, MeshError>> {
        let wait = wait_for_snapshot(&read);
        let mut current = match self.register(id, read)? {
            Ok(current) => current,
            Err(error) => return Some(Err(error)),
        };
        Some(
            match tokio::time::timeout(wait, watched::wait_for(&mut current, Option::is_some)).await
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
    }
}

/// A node that tests make push: the machine's stream is open, and each read is answered by what
/// the test says.
#[cfg(any(test, feature = "test-util"))]
impl Snapshots {
    /// Demands `topic` on `id` and opens its stream, as a view that wants it would, unless it
    /// is open; the stream's generation.
    pub fn open_stream(&self, id: EndpointId, topic: Topic) -> Uuid {
        let mut wanted: BTreeMap<EndpointId, Vec<Topic>> = {
            let held = self.held.lock().unwrap_or_else(PoisonError::into_inner);

            if let Some(open) = held
                .machines
                .get(&id)
                .and_then(|machine| machine.streams.get(&topic))
            {
                return *open;
            }
            held.machines
                .iter()
                .map(|(id, machine)| (*id, machine.topics.clone()))
                .collect()
        };
        let topics = wanted.entry(id).or_default();

        if !topics.contains(&topic) {
            topics.push(topic.clone());
        }
        self.reconcile(&wanted);
        let generation = Uuid::new_v4();
        self.connected(id, topic, generation);
        generation
    }

    /// Has the node push `value` as the answer to `read`.
    pub fn push(&self, id: EndpointId, read: Read, value: Value) {
        self.answer(id, read, |query, topic| Snapshot {
            topic,
            query: Some(query),
            revision: 1,
            value,
        });
    }

    /// Has the node fail `read` with `reason`.
    pub fn refuse(&self, id: EndpointId, read: Read, reason: &str) {
        let (query, generation) = self.query_of(id, read);
        self.failed(id, generation, query, reason.to_owned());
    }

    fn answer(&self, id: EndpointId, read: Read, snapshot: impl FnOnce(u64, Topic) -> Snapshot) {
        let topic = read.topic().expect("a read that has a topic");
        let (query, generation) = self.query_of(id, read);
        self.install(id, generation, &snapshot(query, topic));
    }

    /// The query of `read` on `id`, registered if it is not yet, and its stream's generation.
    fn query_of(&self, id: EndpointId, read: Read) -> (u64, Uuid) {
        drop(self.register(id, read.clone()).expect("an open stream"));
        let held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let machine = &held.machines[&id];
        let entry = machine
            .entries
            .values()
            .find(|entry| entry.query.read == read)
            .expect("a registered query");
        (entry.query.id, machine.streams[&entry.stream])
    }
}

#[cfg(test)]
#[path = "tests/snapshots.rs"]
mod tests;
