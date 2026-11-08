use super::*;
use beep_proto::node::events::{Query, Read, Snapshot, Value};

const MAX_READS: usize = 4;
const MAX_VALUE_BYTES: usize = 4 * 1024 * 1024;
const MAX_RETAINED_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
enum Source {
    Read(Read),
    Plugin,
    Missing,
    Reviews,
    Agent(String),
}

#[derive(Clone)]
struct Projection {
    topic: Topic,
    query: Option<u64>,
    source: Source,
    previous: Option<Value>,
    bytes: usize,
    dirty: bool,
    running: bool,
}

type Key = (Topic, Option<u64>);
struct Completed {
    generation: u64,
    key: Key,
    result: Result<(Value, usize)>,
}

pub(super) struct Snapshots {
    generation: u64,
    revision: u64,
    entries: BTreeMap<(Topic, Option<u64>), Projection>,
    reads: JoinSet<Completed>,
    next_key: Option<Key>,
}

impl Snapshots {
    pub(super) fn new() -> Self {
        Self {
            generation: 0,
            revision: 0,
            entries: BTreeMap::new(),
            reads: JoinSet::new(),
            next_key: None,
        }
    }

    pub(super) fn contains(&self, topic: &Topic) -> bool {
        self.entries.contains_key(&(topic.clone(), None))
    }

    pub(super) fn validate(queries: &[Query]) -> Result<()> {
        let mut ids = HashSet::new();
        anyhow::ensure!(
            queries.len() <= events::MAX_QUERIES,
            "too many snapshot queries"
        );
        for query in queries {
            anyhow::ensure!(ids.insert(query.id), "duplicate snapshot query id");
            anyhow::ensure!(
                query.read.topic().as_ref() == Some(&query.topic),
                "a snapshot query must name its read-only source topic"
            );
        }
        Ok(())
    }

    pub(super) fn replace(
        &mut self,
        offers: &Offers,
        topics: &[Topic],
        queries: &[Query],
        roots: &BTreeMap<Topic, PathBuf>,
    ) {
        let mut wanted = Vec::new();
        let selected = |name: &str| {
            topics
                .iter()
                .any(|topic| topic.name() == name || topic.name() == "*")
        };
        if selected("agents") {
            wanted.push((
                Topic::new("agents"),
                Source::Read(Read::Sessions(Box::new(SessionsRequest::ListAll {}))),
            ));
        }
        for topic in topics.iter().filter(|topic| topic.name() == "agent") {
            let source = match topic.key().filter(|_| !topic.is_pattern()) {
                Some(session) => Source::Agent(session.to_owned()),
                None => Source::Read(Read::Sessions(Box::new(SessionsRequest::ListAll {}))),
            };
            wanted.push((topic.clone(), source));
        }
        if selected("reviews") {
            wanted.push((Topic::new("reviews"), Source::Reviews));
        }
        if selected("processes") || selected("process-output") {
            wanted.push((
                Topic::new("processes"),
                Source::Read(Read::Process(beep_proto::node::process::Request::List {})),
            ));
        }
        if selected("terminals") || selected("terminal-output") {
            wanted.push((Topic::new("terminals"), Source::Read(Read::TerminalList)));
        }
        if selected("sync") {
            wanted.push((Topic::new("sync"), Source::Read(Read::SyncList)));
        }
        if selected("moves") {
            wanted.push((
                Topic::new("moves"),
                Source::Read(Read::Sessions(Box::new(SessionsRequest::Copies {}))),
            ));
        }
        for (topic, root) in roots {
            if topics.iter().any(|wanted| wanted.matches(topic)) {
                wanted.push((
                    topic.clone(),
                    Source::Read(Read::Sessions(Box::new(SessionsRequest::Folder {
                        root: root.to_string_lossy().into_owned(),
                        path: String::new(),
                        ignored: false,
                        depth: 1,
                    }))),
                ));
            }
        }
        for topic in topics
            .iter()
            .filter(|topic| topic.name() == "files" && !topic.is_pattern())
        {
            if !roots.contains_key(topic) {
                wanted.push((topic.clone(), Source::Missing));
            }
        }
        // Plugin publishers retain one current value per exact topic.
        for topic in offers.agents.events.published() {
            if topics.iter().any(|wanted| wanted.covers(&topic)) {
                wanted.push((topic, Source::Plugin));
            }
        }
        // An exact plugin subscription can precede the plugin's first publication.
        for topic in topics.iter().filter(|topic| {
            !topic.is_pattern()
                && !matches!(
                    topic.name(),
                    "files"
                        | "agents"
                        | "agent"
                        | "processes"
                        | "process-output"
                        | "terminals"
                        | "terminal-output"
                        | "sync"
                        | "moves"
                        | "reviews"
                )
        }) {
            wanted.push((topic.clone(), Source::Plugin));
        }
        let desired: BTreeMap<_, _> = wanted
            .into_iter()
            .map(|(topic, source)| ((topic, None), source))
            .chain(queries.iter().map(|query| {
                (
                    (query.topic.clone(), Some(query.id)),
                    Source::Read(query.read.clone()),
                )
            }))
            .collect();
        if desired.len() == self.entries.len()
            && desired.iter().all(|(key, source)| {
                self.entries
                    .get(key)
                    .is_some_and(|entry| &entry.source == source)
            })
        {
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        self.reads.abort_all();
        let mut next = BTreeMap::new();
        for ((topic, query), source) in desired {
            let key = (topic.clone(), query);
            let previous = self.entries.remove(&key).filter(|old| old.source == source);
            let entry = match previous {
                Some(mut old) => {
                    old.dirty |= old.running;
                    old.running = false;
                    old
                }
                None => Projection {
                    topic,
                    query,
                    source,
                    previous: None,
                    bytes: 0,
                    dirty: true,
                    running: false,
                },
            };
            next.insert(key, entry);
        }
        // A removed wildcard resource needs one authoritative tombstone too.
        for (key, previous) in &self.entries {
            if previous.query.is_none()
                && previous.source != Source::Missing
                && !next.contains_key(key)
                && topics.iter().any(|topic| topic.covers(&previous.topic))
            {
                next.insert(
                    key.clone(),
                    Projection {
                        topic: previous.topic.clone(),
                        query: None,
                        source: Source::Missing,
                        previous: None,
                        bytes: 0,
                        dirty: true,
                        running: false,
                    },
                );
            }
        }
        self.entries = next;
    }

    pub(super) fn changed(&mut self, topic: &Topic) {
        for entry in self.entries.values_mut() {
            if affected(&entry.topic, topic) {
                entry.dirty = true;
            }
        }
    }

    pub(super) fn reset(&mut self) {
        for entry in self.entries.values_mut() {
            entry.dirty = true;
            entry.previous = None;
            entry.bytes = 0;
        }
    }

    pub(super) fn start(&mut self, offers: &Offers, caller: iroh::EndpointId) {
        // Round robin prevents a busy low-sorted topic from starving other projections.
        let mut keys: Vec<_> = self.entries.keys().cloned().collect();
        if let Some(previous) = &self.next_key {
            let at = keys.partition_point(|key| key <= previous);
            keys.rotate_left(at);
        }
        for key in keys {
            if self.reads.len() >= MAX_READS {
                break;
            }
            let Some(entry) = self.entries.get_mut(&key) else {
                continue;
            };
            if !entry.dirty || entry.running {
                continue;
            }
            self.next_key = Some(key.clone());
            entry.dirty = false;
            entry.running = true;
            let source = entry.source.clone();
            let key = key.clone();
            let offers = offers.clone();
            let generation = self.generation;
            self.reads.spawn(async move {
                let result = timeout(Duration::from_secs(60), async {
                    match source {
                        Source::Read(read) => read_value(offers, caller, read).await,
                        Source::Agent(session) => {
                            let value = read_value(
                                offers,
                                caller,
                                Read::Sessions(Box::new(SessionsRequest::ListAll {})),
                            )
                            .await?;
                            match value {
                                Value::Sessions(reply) => match *reply {
                                    SessionsReply::Sessions { sessions } => {
                                        Ok(Value::Sessions(Box::new(SessionsReply::Sessions {
                                            sessions: sessions
                                                .into_iter()
                                                .filter(|info| info.id == session)
                                                .collect(),
                                        })))
                                    }
                                    other => Ok(Value::Sessions(Box::new(other))),
                                },
                                other => Ok(other),
                            }
                        }
                        Source::Reviews => Ok(Value::Sessions(Box::new(SessionsReply::Reviews {
                            runs: offers.agents.review_snapshots(),
                        }))),
                        Source::Missing => Ok(Value::Missing),
                        Source::Plugin => offers
                            .agents
                            .events
                            .current(&key.0)
                            .context("no state has been published for this topic"),
                    }
                })
                .await
                .context("snapshot read timed out")
                .and_then(|r| r);
                let result = match result {
                    Ok(value) => tokio::task::spawn_blocking(move || {
                        let bytes = rmp_serde::to_vec(&value)?.len();
                        anyhow::ensure!(
                            bytes <= MAX_VALUE_BYTES,
                            "snapshot is too large; request a smaller projection"
                        );
                        Ok((value, bytes))
                    })
                    .await
                    .context("snapshot encoding stopped")
                    .and_then(|result| result),
                    Err(error) => Err(error),
                };
                Completed {
                    generation,
                    key,
                    result,
                }
            });
        }
    }

    pub(super) async fn next(&mut self) -> Option<Reply> {
        let Some(result) = self.reads.join_next().await else {
            return std::future::pending().await;
        };
        let Ok(Completed {
            generation,
            key,
            result,
        }) = result
        else {
            return None;
        };
        if generation != self.generation {
            return None;
        }
        let retained: usize = self.entries.values().map(|entry| entry.bytes).sum();
        let entry = self.entries.get_mut(&key)?;
        entry.running = false;
        Some(match result {
            Ok((value, bytes)) => {
                if entry.previous.as_ref() == Some(&value) {
                    return None;
                }
                if retained.saturating_sub(entry.bytes).saturating_add(bytes) > MAX_RETAINED_BYTES {
                    entry.previous = None;
                    entry.bytes = 0;
                    return Some(Reply::SnapshotError {
                        query: entry.query,
                        topic: entry.topic.clone(),
                        reason: "snapshot cache budget exceeded; narrow the subscription".into(),
                    });
                }
                entry.bytes = bytes;
                entry.previous = Some(value.clone());
                self.revision = self.revision.checked_add(1)?;
                Reply::Snapshot(Box::new(Snapshot {
                    topic: entry.topic.clone(),
                    query: entry.query,
                    revision: self.revision,
                    value,
                }))
            }
            Err(error) => {
                entry.previous = None;
                entry.bytes = 0;
                Reply::SnapshotError {
                    query: entry.query,
                    topic: entry.topic.clone(),
                    reason: format!("{error:#}"),
                }
            }
        })
    }
}

fn affected(read: &Topic, change: &Topic) -> bool {
    read.matches(change)
        || matches!(
            (read.name(), change.name()),
            ("agents", "agent" | "moves")
                | ("agent", "agents")
                | ("processes", "process-output")
                | ("terminals", "terminal-output")
        )
}

async fn read_value(offers: Offers, caller: iroh::EndpointId, read: Read) -> Result<Value> {
    match read {
        Read::Sessions(request) => {
            match execute(offers.agents, caller, Operation::Sessions(*request)).await? {
                CallReply::Sessions(reply) => Ok(Value::Sessions(reply)),
                _ => bail!("unexpected snapshot reply"),
            }
        }
        Read::Process(request) => Ok(Value::Process(
            crate::run::tasks::answer(crate::run::tasks::Table::Shared, request).await,
        )),
        Read::TerminalList => {
            let link = beep_proto::node::local_link();
            let _task = AbortOnDropHandle::new(tokio::spawn(crate::run::terminal::serve(
                link.node.0,
                link.node.1,
            )));
            let (mut tx, mut rx) = link.caller;
            tx.send(&beep_proto::node::terminal::Request::List {})
                .await?;
            Ok(Value::Terminals(
                rx.recv()
                    .await?
                    .context("terminal list closed without state")?,
            ))
        }
        Read::SyncList => Ok(Value::Sync(Box::new(
            beep_proto::node::sync::Reply::Status {
                projects: offers
                    .transfers
                    .get()
                    .map(|t| t.syncs().status())
                    .unwrap_or_default(),
            },
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn recovery_after_a_read_error_resends_identical_state() {
        let mut snapshots = Snapshots::new();
        let topic = Topic::keyed("plugin", "example");
        let key = (topic.clone(), Some(7));
        let value = Value::Plugin(vec![1]);
        snapshots.entries.insert(
            key.clone(),
            Projection {
                topic,
                query: Some(7),
                source: Source::Plugin,
                previous: Some(value.clone()),
                bytes: 1,
                dirty: false,
                running: true,
            },
        );
        let failed = key.clone();
        snapshots.reads.spawn(async move {
            Completed {
                generation: 0,
                key: failed,
                result: Err(anyhow::anyhow!("temporary read failure")),
            }
        });
        assert!(matches!(
            snapshots.next().await,
            Some(Reply::SnapshotError { query: Some(7), .. })
        ));
        let entry = snapshots.entries.get(&key).unwrap();
        assert!(entry.previous.is_none());
        assert_eq!(entry.bytes, 0);
        assert!(!entry.running);
        let restored = value.clone();
        snapshots.reads.spawn(async move {
            Completed {
                generation: 0,
                key,
                result: Ok((restored, 1)),
            }
        });
        let Some(Reply::Snapshot(snapshot)) = snapshots.next().await else {
            panic!("expected recovered state")
        };
        assert_eq!(snapshot.value, value);
        assert_eq!(snapshot.query, Some(7));
        assert_eq!(snapshot.revision, 1);
    }
}
