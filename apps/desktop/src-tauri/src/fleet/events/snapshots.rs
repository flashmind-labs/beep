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
    owners: Vec<Topic>,
    used: u64,
    current: watch::Sender<Current>,
}

struct Machine {
    topics: Vec<Topic>,
    generation: Option<Uuid>,
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
                generation: None,
                entries: BTreeMap::new(),
            });
            if machine.topics != *topics {
                changed = true;
                machine.topics.clone_from(topics);
            }
            let before = machine.entries.len();
            machine.entries.retain(|_, entry| {
                entry.owners = topics
                    .iter()
                    .filter(|wanted| {
                        wanted.matches(&entry.query.topic)
                            || entry.owners.iter().any(|owner| wanted.covers(owner))
                    })
                    .cloned()
                    .collect();
                !entry.owners.is_empty()
            });
            changed |= before != machine.entries.len();
        }
        drop(held);
        if changed {
            self.changed.send_replace(());
        }
    }

    pub(super) fn begin(&self, id: EndpointId, generation: Uuid) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(machine) = held.machines.get_mut(&id) {
            machine.generation = Some(generation);
            for entry in machine.entries.values() {
                entry.current.send_replace(None);
            }
        }
    }

    pub(super) fn end(&self, id: EndpointId, generation: Uuid) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(machine) = held.machines.get_mut(&id)
            && machine.generation == Some(generation)
        {
            machine.generation = None;
            for entry in machine.entries.values() {
                entry.current.send_replace(None);
            }
        }
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
            .filter(|machine| machine.generation == Some(generation))?;
        let Some(query) = snapshot.query else {
            return Some(false);
        };
        if let Some(entry) = machine.entries.get(&query)
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
            && machine.generation == Some(generation)
            && let Some(entry) = machine.entries.get(&query)
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
        let owners: Vec<_> = machine
            .topics
            .iter()
            .filter(|wanted| demanded(&topic, std::slice::from_ref(wanted)))
            .cloned()
            .collect();
        if owners.is_empty() {
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
                entry.owners = owners;
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
                owners,
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
                    Ok(Err(_)) => Err(error("the subscription was removed")),
                    Err(_) => Err(error("the node did not provide its subscription snapshot")),
                },
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::node::{process, terminal};

    fn demanded_machine(snapshots: &Snapshots, topic: &str) -> (EndpointId, Uuid) {
        let id = iroh::SecretKey::generate().public();
        snapshots.reconcile(&BTreeMap::from([(id, vec![Topic::new(topic)])]));
        let generation = Uuid::new_v4();
        snapshots.begin(id, generation);
        (id, generation)
    }

    fn terminal_snapshot(query: &Query) -> Snapshot {
        Snapshot {
            topic: query.topic.clone(),
            query: Some(query.id),
            revision: 1,
            value: Value::Terminals(terminal::Reply::Terminals {
                terminals: Vec::new(),
            }),
        }
    }

    #[tokio::test]
    async fn subscribed_reads_wait_for_the_first_snapshot_and_then_use_current_state() {
        let snapshots = Arc::new(Snapshots::default());
        let (id, generation) = demanded_machine(&snapshots, "terminals");
        let reader = snapshots.reader(id);
        let first = reader.snapshot(Read::TerminalList);
        tokio::pin!(first);
        assert!(futures::poll!(&mut first).is_pending());
        let query = snapshots.queries(id).pop().unwrap();
        let snapshot = terminal_snapshot(&query);
        assert_eq!(snapshots.install(id, generation, &snapshot), Some(true));
        assert_eq!(first.await.unwrap().unwrap(), snapshot.value);
        assert_eq!(
            reader.snapshot(Read::TerminalList).await.unwrap().unwrap(),
            snapshot.value
        );
        assert_eq!(snapshots.queries(id), [query]);
    }

    #[test]
    fn reconnect_clears_values_and_rejects_late_snapshots_from_the_previous_stream() {
        let snapshots = Snapshots::default();
        let (id, generation) = demanded_machine(&snapshots, "terminals");
        let current = snapshots.register(id, Read::TerminalList).unwrap().unwrap();
        let query = snapshots.queries(id).pop().unwrap();
        let snapshot = terminal_snapshot(&query);
        assert_eq!(snapshots.install(id, generation, &snapshot), Some(true));
        assert!(current.borrow().is_some());
        let replacement = Uuid::new_v4();
        snapshots.begin(id, replacement);
        assert!(current.borrow().is_none());
        assert_eq!(snapshots.install(id, generation, &snapshot), None);
        assert!(current.borrow().is_none());
        snapshots.end(id, generation);
        assert_eq!(snapshots.install(id, replacement, &snapshot), Some(true));
        assert!(current.borrow().is_some());
    }

    #[test]
    fn removing_demand_drops_queries_and_wakes_pending_readers() {
        let snapshots = Snapshots::default();
        let (id, _) = demanded_machine(&snapshots, "terminals");
        let current = snapshots.register(id, Read::TerminalList).unwrap().unwrap();
        snapshots.reconcile(&BTreeMap::new());
        assert!(current.has_changed().is_err());
        assert!(snapshots.queries(id).is_empty());
        assert!(snapshots.register(id, Read::TerminalList).is_none());
        assert!(
            snapshots
                .register(
                    id,
                    Read::Process(process::Request::Start {
                        command: "true".into(),
                        cwd: None
                    })
                )
                .is_none()
        );
    }

    #[test]
    fn query_storage_evicts_the_least_recent_read_and_keeps_reused_projections() {
        let snapshots = Snapshots::default();
        let (id, _) = demanded_machine(&snapshots, "process-output");
        let first = Read::Process(process::Request::Status {
            id: process::TaskId(0),
        });
        for number in 0..u32::try_from(events::MAX_QUERIES).unwrap() {
            snapshots
                .register(
                    id,
                    Read::Process(process::Request::Status {
                        id: process::TaskId(number),
                    }),
                )
                .unwrap()
                .unwrap();
        }
        snapshots.register(id, first.clone()).unwrap().unwrap();
        snapshots
            .register(
                id,
                Read::Process(process::Request::Status {
                    id: process::TaskId(u32::MAX),
                }),
            )
            .unwrap()
            .unwrap();
        let queries = snapshots.queries(id);
        assert_eq!(queries.len(), events::MAX_QUERIES);
        assert!(queries.iter().any(|query| query.read == first));
        assert!(!queries.iter().any(|query| query.read
            == Read::Process(process::Request::Status {
                id: process::TaskId(1)
            })));
    }

    #[test]
    fn a_failed_read_registers_a_new_projection_on_the_next_attempt() {
        let snapshots = Snapshots::default();
        let (id, generation) = demanded_machine(&snapshots, "terminals");
        let first = snapshots.register(id, Read::TerminalList).unwrap().unwrap();
        let query = snapshots.queries(id).pop().unwrap();
        snapshots.failed(id, generation, query.id, "temporary read failure".into());
        assert!(matches!(first.borrow().as_ref(), Some(Err(_))));
        let retry = snapshots.register(id, Read::TerminalList).unwrap().unwrap();
        let replacement = snapshots.queries(id).pop().unwrap();
        assert_ne!(query.id, replacement.id);
        assert!(retry.borrow().is_none());
        assert_eq!(
            snapshots.install(id, generation, &terminal_snapshot(&replacement)),
            Some(true)
        );
        assert!(matches!(retry.borrow().as_ref(), Some(Ok(_))));
    }

    #[test]
    fn switching_an_exact_file_root_releases_its_previous_projection() {
        use beep_proto::node::agent::SessionsRequest;
        let snapshots = Snapshots::default();
        let id = iroh::SecretKey::generate().public();
        let root = "/srv/project-a";
        snapshots.reconcile(&BTreeMap::from([(id, vec![Topic::keyed("files", root)])]));
        let read = Read::Sessions(Box::new(SessionsRequest::Folder {
            root: root.into(),
            path: String::new(),
            ignored: false,
            depth: 1,
        }));
        let previous = snapshots.register(id, read.clone()).unwrap().unwrap();
        snapshots.reconcile(&BTreeMap::from([(
            id,
            vec![Topic::keyed("files", "/srv/project-b")],
        )]));
        assert!(previous.has_changed().is_err());
        assert!(snapshots.queries(id).is_empty());
        assert!(snapshots.register(id, read).is_none());
    }

    #[test]
    fn switching_a_project_alias_releases_its_physical_file_projection() {
        use beep_proto::node::agent::SessionsRequest;
        let snapshots = Snapshots::default();
        let id = iroh::SecretKey::generate().public();
        snapshots.reconcile(&BTreeMap::from([(
            id,
            vec![Topic::parse("files.org.project-a").unwrap()],
        )]));
        let read = Read::Sessions(Box::new(SessionsRequest::Folder {
            root: "/srv/project-a".into(),
            path: String::new(),
            ignored: false,
            depth: 1,
        }));
        let previous = snapshots.register(id, read).unwrap().unwrap();
        snapshots.reconcile(&BTreeMap::from([(
            id,
            vec![Topic::parse("files.org.project-b").unwrap()],
        )]));
        assert!(previous.has_changed().is_err());
        assert!(snapshots.queries(id).is_empty());
    }

    #[test]
    fn the_first_query_value_is_distinct_from_live_updates_and_error_recovery() {
        let snapshots = Snapshots::default();
        let (id, generation) = demanded_machine(&snapshots, "terminals");
        let _current = snapshots.register(id, Read::TerminalList).unwrap().unwrap();
        let query = snapshots.queries(id).pop().unwrap();
        let snapshot = terminal_snapshot(&query);
        assert_eq!(snapshots.install(id, generation, &snapshot), Some(true));
        assert_eq!(snapshots.install(id, generation, &snapshot), Some(false));
        assert!(snapshots.failed(id, generation, query.id, "temporary failure".into()));
        assert_eq!(snapshots.install(id, generation, &snapshot), Some(false));
        let topic = Snapshot {
            query: None,
            ..snapshot
        };
        assert_eq!(snapshots.install(id, generation, &topic), Some(false));
    }
}
