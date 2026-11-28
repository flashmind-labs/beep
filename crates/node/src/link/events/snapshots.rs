use super::ask::{ask, execute};
use crate::events::{ChangeKind, Touched};
use crate::link::Offers;
use anyhow::{Context, Result, bail};
use beep_proto::SessionId;
use beep_proto::node::agent::SessionInfo;
use beep_proto::node::process::TaskId;
use beep_proto::node::subscriptions::Reply;
use beep_proto::node::{
    agent::{SessionsReply, SessionsRequest},
    events::{
        self, CallReply, ExternalDelta, FORGE_REACH_TOPIC, MOVE_PROGRESS_TOPIC, Operation, Query,
        Read, SessionsDelta, Snapshot, TEAM_CARDS_TOPIC, TEAM_INVITES_TOPIC, Topic, Value,
    },
};
use beep_proto::{node::device::Device, watched};
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
    time::Duration,
};
use tokio::sync::watch;
use tokio::{
    task::JoinSet,
    time::{Instant, timeout},
};

const MAX_READS: usize = 4;
const MAX_VALUE_BYTES: usize = 4 * 1024 * 1024;
const MAX_RETAINED_BYTES: usize = 16 * 1024 * 1024;
/// The least time between two reads of one projection. A change during the
/// gap or a read marks it dirty, so the last state is always read after it.
const READ_GAP: Duration = Duration::from_millis(250);
/// One session's status, question and tasks follow its turn more closely.
/// Its chat text streams separately and never waits for this gap.
const AGENT_GAP: Duration = Duration::from_millis(100);
/// Lists of the sessions agents ran outside Beep scan what each agent keeps on
/// disk, and change on every line a running agent writes; they only start or
/// end sessions, so they wait longer between reads.
const SCAN_GAP: Duration = Duration::from_secs(2);
/// The checkouts sessions worked in: a whole look runs git for every folder
/// and only changes when a session starts in a new one. Desktops up to 0.8.3
/// kept this list live after the Add a project sheet asked once.
const FOLDERS_GAP: Duration = Duration::from_secs(60);
/// One outside session looked up by its id. Desktops up to 0.8.4 kept such a
/// lookup live for every session row they drew, and the node looked again
/// after every line any agent wrote; where a session works rarely changes.
const SESSION_GAP: Duration = Duration::from_secs(30);
/// A desktop's file read that came back unchanged or failed waits twice as
/// long before the next, up to this many times: desktops up to 0.8.4 kept
/// one-off reads live, and nothing tells the node whether anyone still reads
/// them.
const MAX_IDLE_DOUBLINGS: u32 = 3;

#[derive(Clone, Debug, PartialEq)]
enum Projected {
    Read(Read),
    Plugin,
    Missing,
    Reviews,
    Agent(SessionId),
    Resources(Vec<Topic>),
    Devices,
    ToolUpdates,
    /// How this node's calls to each code host went.
    ForgeReach,
    /// The cards of the team of the project with this name.
    TeamCards(String),
    /// The invites waiting for a desktop on this node.
    TeamInvites,
    /// One terminal, as the list of them holds it.
    Terminal(TaskId),
    /// One background task: its state and last lines.
    Task(TaskId),
    /// How each move to this node stands.
    Moves,
}

impl Projected {
    fn gap(&self) -> Duration {
        match self {
            Self::Agent(_) => AGENT_GAP,
            Self::Read(Read::Sessions(request))
                if matches!(
                    request.as_ref(),
                    SessionsRequest::External {
                        session: Some(_),
                        ..
                    }
                ) =>
            {
                SESSION_GAP
            }
            Self::Read(Read::Sessions(request))
                if matches!(
                    request.as_ref(),
                    SessionsRequest::External { .. } | SessionsRequest::InFolders { .. }
                ) =>
            {
                SCAN_GAP
            }
            Self::Read(Read::Sessions(request))
                if matches!(request.as_ref(), SessionsRequest::Folders { .. }) =>
            {
                FOLDERS_GAP
            }
            _ => READ_GAP,
        }
    }

    /// Reads of a folder's files that wait longer while they come back unchanged.
    fn backs_off(&self) -> bool {
        match self {
            Self::Read(Read::Git { .. }) => true,
            Self::Read(Read::Sessions(request)) => matches!(
                request.as_ref(),
                SessionsRequest::Folder { .. }
                    | SessionsRequest::ReadFile { .. }
                    | SessionsRequest::Diff { .. }
                    | SessionsRequest::Worktrees { .. }
                    | SessionsRequest::Present { .. }
                    | SessionsRequest::Commands { .. }
            ),
            _ => false,
        }
    }
}

#[derive(Clone)]
struct Projection {
    source: Projected,
    previous: Option<Value>,
    bytes: usize,
    dirty: bool,
    running: bool,
    /// What changed in a folder since its last read began, for its next read to say.
    touched: Touched,
    /// What the read that runs now was started for.
    reading: Touched,
    not_before: Instant,
    /// Reads in a row that gave the value before or failed.
    idle: u32,
    /// The revision of the last message sent for it, 0 before the first and
    /// after anything that makes the next one whole.
    sent: u64,
}

impl Projection {
    fn new(source: Projected) -> Self {
        Self {
            source,
            previous: None,
            bytes: 0,
            dirty: true,
            running: false,
            touched: Touched::All,
            reading: Touched::All,
            not_before: Instant::now(),
            idle: 0,
            sent: 0,
        }
    }

    /// The least time to the next read. A file read, a desktop's or the
    /// listing behind a folder's `files` topic, backs off while it comes back
    /// unchanged or fails: an agent writing in a checkout would otherwise run
    /// git status there four times a second.
    fn gap(&self) -> Duration {
        let gap = self.source.gap();

        if !self.source.backs_off() {
            return gap;
        }

        gap * 2_u32.pow(self.idle.min(MAX_IDLE_DOUBLINGS))
    }
}

type Key = (Topic, Option<u64>);
struct Completed {
    generation: u64,
    key: Key,
    result: Result<(Value, usize)>,
}

pub(super) struct Snapshots {
    stream: uuid::Uuid,
    generation: u64,
    revision: u64,
    /// The follower applies `Value::SessionsDelta`.
    deltas: bool,
    /// The follower applies `Value::ExternalDelta` on external-session queries.
    query_deltas: bool,
    entries: BTreeMap<Key, Projection>,
    reads: JoinSet<Completed>,
    /// The device list the follow of the `devices` topics stores.
    devices: watch::Receiver<Option<Vec<Device>>>,
    next_key: Option<Key>,
}

impl Snapshots {
    pub(super) fn new() -> Self {
        Self {
            stream: uuid::Uuid::new_v4(),
            generation: 0,
            revision: 0,
            deltas: false,
            query_deltas: false,
            entries: BTreeMap::new(),
            reads: JoinSet::new(),
            devices: crate::run::device::stored(),
            next_key: None,
        }
    }

    /// Whether the follower takes the `agents` list as deltas. A change of
    /// mind starts every projection over with a whole value.
    pub(super) fn follow_deltas(&mut self, deltas: bool) {
        if self.deltas != deltas {
            self.deltas = deltas;
            self.reset();
        }
    }

    /// Whether the follower takes external-session queries as deltas.
    pub(super) fn follow_query_deltas(&mut self, query_deltas: bool) {
        if self.query_deltas != query_deltas {
            self.query_deltas = query_deltas;
            self.reset();
        }
    }

    pub(super) fn stream(&self) -> uuid::Uuid {
        self.stream
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
                Projected::Read(Read::Sessions(Box::new(SessionsRequest::ListAll {}))),
            ));
        }

        wanted.extend(topics_named(topics, "agent").map(|topic| {
            let source = match topic.key().filter(|_| !topic.is_pattern()) {
                Some(session) => Projected::Agent(Into::<SessionId>::into(session)),
                None => Projected::Read(Read::Sessions(Box::new(SessionsRequest::ListAll {}))),
            };

            (topic.clone(), source)
        }));

        if selected("reviews") {
            wanted.push((Topic::new("reviews"), Projected::Reviews));
        }
        wanted.extend(topics_named(topics, "processes").filter_map(|topic| {
            let session = topic.key().filter(|_| !topic.is_pattern())?;
            let request = SessionsRequest::Processes {
                session: Into::<SessionId>::into(session),
            };

            Some((
                topic.clone(),
                Projected::Read(Read::Sessions(Box::new(request))),
            ))
        }));

        // A topic naming one task gets that task alone.
        for topic in topics {
            if let Some(id) = numbered(topic, "process-output") {
                wanted.push((topic.clone(), Projected::Task(id)));
            }
        }

        if topics.iter().any(|topic| {
            topic.name() == "*"
                || (topic.name() == "process-output" && numbered(topic, "process-output").is_none())
                || (topic.name() == "processes" && topic.is_pattern())
        }) {
            wanted.push((
                Topic::new("processes"),
                Projected::Read(Read::Process(beep_proto::node::process::Request::List {})),
            ));
        }
        // A topic naming one terminal gets that terminal alone; the others,
        // for desktops from before, get the whole list.
        let one = |topic: &Topic| numbered(topic, "terminal-output");
        let listed = topics.iter().any(|topic| match topic.name() {
            "terminals" | "*" => true,
            "terminal-output" => one(topic).is_none(),
            _ => false,
        });
        if listed {
            wanted.push((Topic::new("terminals"), Projected::Read(Read::TerminalList)));
        }
        for topic in topics {
            if let Some(id) = one(topic) {
                wanted.push((topic.clone(), Projected::Terminal(id)));
            }
        }

        // These carry values released readers cannot decode, so `*` leaves them out.
        wanted.extend(
            topics_named(topics, crate::run::device::LIST_TOPIC)
                .map(|t| (t.clone(), Projected::Devices)),
        );

        // Only a topic keyed by a project's name has cards: they are not for `*`.
        wanted.extend(topics_named(topics, TEAM_CARDS_TOPIC).filter_map(|topic| {
            let project = topic.key().filter(|_| !topic.is_pattern())?;

            Some((topic.clone(), Projected::TeamCards(project.to_owned())))
        }));
        wanted.extend(
            topics_named(topics, TEAM_INVITES_TOPIC).map(|t| (t.clone(), Projected::TeamInvites)),
        );
        wanted.extend(
            topics_named(topics, FORGE_REACH_TOPIC).map(|t| (t.clone(), Projected::ForgeReach)),
        );
        wanted.extend(
            topics_named(topics, crate::machine::provision::UPDATES_TOPIC)
                .map(|t| (t.clone(), Projected::ToolUpdates)),
        );
        if selected("sign-in") {
            for tool in [
                beep_proto::node::sign_in::Tool::Claude,
                beep_proto::node::sign_in::Tool::Codex,
                beep_proto::node::sign_in::Tool::Opencode,
                beep_proto::node::sign_in::Tool::Agy,
            ] {
                let topic = Topic::keyed("sign-in", tool.to_string());

                if topics.iter().any(|wanted| wanted.covers(&topic)) {
                    wanted.push((
                        topic,
                        Projected::Read(Read::SignIn {
                            tool: tool.to_string(),
                        }),
                    ));
                }
            }
        }

        if selected("sync") {
            wanted.push((Topic::new("sync"), Projected::Read(Read::SyncList)));
        }
        // Only a topic that names it gets it, never `*`.
        if topics
            .iter()
            .any(|topic| topic.name() == MOVE_PROGRESS_TOPIC)
        {
            wanted.push((Topic::new(MOVE_PROGRESS_TOPIC), Projected::Moves));
        }
        if selected("moves") {
            wanted.push((
                Topic::new("moves"),
                Projected::Read(Read::Sessions(Box::new(SessionsRequest::Copies {}))),
            ));
        }

        for topic in topics
            .iter()
            .filter(|topic| topic.is_pattern() && matches!(topic.name(), "files" | "*"))
        {
            let scope = if topic.name() == "*" {
                Topic::new("files")
            } else {
                topic.clone()
            };
            let resources = roots
                .keys()
                .filter(|topic| scope.covers(topic))
                .cloned()
                .collect();
            wanted.push((scope, Projected::Resources(resources)));
        }

        for (topic, root) in roots {
            if topics.iter().any(|wanted| wanted.matches(topic)) {
                wanted.push((
                    topic.clone(),
                    Projected::Read(Read::Sessions(Box::new(SessionsRequest::Folder {
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
                wanted.push((topic.clone(), Projected::Missing));
            }
        }
        // Plugin publishers retain one current value per exact topic.
        for topic in offers.agents.events.published() {
            if topics.iter().any(|wanted| wanted.covers(&topic)) {
                wanted.push((topic, Projected::Plugin));
            }
        }
        // An exact plugin subscription can precede the plugin's first publication.
        for topic in topics
            .iter()
            .filter(|topic| !topic.is_pattern() && !PROJECTED.contains(&topic.name()))
        {
            wanted.push((topic.clone(), Projected::Plugin));
        }
        let desired: BTreeMap<_, _> = wanted
            .into_iter()
            .map(|(topic, source)| ((topic, None), source))
            .chain(queries.iter().map(|query| {
                (
                    (query.topic.clone(), Some(query.id)),
                    Projected::Read(query.read.clone()),
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
        tracing::info!(stream = %self.stream, generation = self.generation, pending_reads = self.reads.len(), projections = desired.len(), "subscription projections replaced");
        self.reads.abort_all();
        let mut next = BTreeMap::new();

        for ((topic, query), source) in desired {
            let key = (topic.clone(), query);
            let previous = self.entries.remove(&key).filter(|old| old.source == source);
            let entry = match previous {
                Some(mut old) => {
                    if old.running {
                        old.dirty = true;
                        old.touched = Touched::All;
                    }

                    old.running = false;
                    old
                }
                None => Projection::new(source),
            };
            next.insert(key, entry);
        }
        // A removed wildcard resource needs one authoritative tombstone too.
        for (key, previous) in &self.entries {
            if key.1.is_none()
                && previous.source != Projected::Missing
                && !next.contains_key(key)
                && topics.iter().any(|topic| topic.covers(&key.0))
            {
                next.insert(key.clone(), Projection::new(Projected::Missing));
            }
        }

        self.entries = next;
    }

    pub(super) fn changed(&mut self, topic: &Topic, kind: ChangeKind, touched: &Touched) {
        for ((read, _), entry) in &mut self.entries {
            // Git projections read refs, the index and history, never working-tree files.
            let git = matches!(entry.source, Projected::Read(Read::Git { .. }));

            if affected_entry(read, &entry.source, topic) && !(git && kind == ChangeKind::WorkTree)
            {
                entry.dirty = true;

                // Only the folder's own change says which of its paths changed.
                if read == topic {
                    entry.touched.add(touched.clone());
                } else {
                    entry.touched = Touched::All;
                }
            }
        }
    }

    pub(super) fn reset(&mut self) {
        for entry in self.entries.values_mut() {
            entry.dirty = true;
            entry.touched = Touched::All;
            entry.previous = None;
            entry.bytes = 0;
            entry.sent = 0;
        }
    }

    pub(super) fn start(&mut self, offers: &Offers, caller: iroh::EndpointId) {
        let now = Instant::now();
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

            if !entry.dirty || entry.running || entry.not_before > now {
                continue;
            }

            self.next_key = Some(key.clone());
            entry.dirty = false;
            entry.running = true;
            entry.reading = std::mem::replace(&mut entry.touched, Touched::Nothing);
            entry.not_before = now + entry.gap();
            let source = entry.source.clone();
            let key = key.clone();
            metrics::counter!("beep_snapshot_reads_total", "topic" => key.0.name().to_owned())
                .increment(1);
            let offers = offers.clone();
            let mut devices = self.devices.clone();
            let generation = self.generation;
            let stream = self.stream;
            let listed = key.clone();
            tracing::debug!(%stream, generation, query = key.1, topic = key.0.name(), "subscription snapshot scheduled");
            self.reads.spawn(async move {
                let started = Instant::now();
                tracing::debug!(%stream, generation, query = key.1, topic = key.0.name(), "subscription snapshot read started");
                let result = timeout(Duration::from_secs(60), async {
                    match source {
                        Projected::Read(read) => read_value(offers, caller, read).await,
                        Projected::Agent(session) => {
                            Ok(Value::Sessions(Box::new(SessionsReply::Sessions {
                                sessions: offers.agents.sessions.listed_session(&session),
                            })))
                        }
                        Projected::Reviews => Ok(Value::Sessions(Box::new(SessionsReply::Reviews {
                            runs: offers.agents.reviews.review_snapshots(),
                        }))),
                        Projected::Missing => Ok(Value::Missing),
                        Projected::Resources(resources) => Ok(Value::Resources(resources)),
                        Projected::Devices => {
                            let listed = match watched::wait_for(&mut devices, Option::is_some).await {
                                Ok(()) => watched::read(&devices, Clone::clone).unwrap_or_default(),
                                Err(_) => Vec::new(),
                            };

                            Ok(Value::Devices(listed))
                        }
                        Projected::TeamCards(project) => Ok(offers
                            .team
                            .as_ref()
                            .and_then(|team| crate::team::apps::cards_of(team, &project))
                            .map_or(Value::Missing, Value::TeamCards)),
                        Projected::Terminal(id) => {
                            let list = ask(
                                &beep_proto::node::terminal::Request::List {},
                                crate::run::terminal::serve,
                            )
                            .await?;

                            Ok(match list.context("terminal list closed without state")? {
                                beep_proto::node::terminal::Reply::Terminals { terminals } => terminals
                                    .into_iter()
                                    .find(|task| task.id == id)
                                    .map_or(Value::Missing, |task| {
                                        Value::Terminals(beep_proto::node::terminal::Reply::Terminals {
                                            terminals: vec![task],
                                        })
                                    }),
                                other => Value::Terminals(other),
                            })
                        }
                        Projected::Task(id) => Ok(Value::Process(
                            crate::run::tasks::answer(
                                crate::run::tasks::Table::Shared,
                                beep_proto::node::process::Request::Status { id },
                            )
                            .await,
                        )),
                        Projected::TeamInvites => Ok(offers
                            .team
                            .as_ref()
                            .map_or(Value::Missing, |team| Value::TeamInvites(team.invites_waiting()))),
                        Projected::Moves => Ok(Value::Sessions(Box::new(SessionsReply::Moves {
                            moves: offers.agents.moves.entries(),
                        }))),
                        Projected::ForgeReach => Ok(Value::ForgeReach(offers.agents.ci.reach())),
                        Projected::ToolUpdates => Ok(offers
                            .provision
                            .as_ref()
                            .and_then(|provision| provision.updatable())
                            .map_or(Value::Missing, Value::ToolUpdates)),
                        Projected::Plugin => offers
                            .agents
                            .events
                            .current(&key.0)
                            .context("no state has been published for this topic"),
                    }
                })
                .await
                .context("snapshot read timed out")
                .and_then(|r| r);
                tracing::debug!(%stream, generation, query = key.1, topic = key.0.name(), success = result.is_ok(), elapsed_ms = started.elapsed().as_millis() as u64, "subscription snapshot source completed");
                let result = match result {
                    Ok(value) => tokio::task::spawn_blocking(move || {
                        let value = shown(&listed, value);
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
                tracing::debug!(%stream, generation, query = key.1, topic = key.0.name(), success = result.is_ok(), bytes = result.as_ref().map(|(_, bytes)| *bytes).unwrap_or(0), elapsed_ms = started.elapsed().as_millis() as u64, "subscription snapshot read completed");
                Completed {
                    generation,
                    key,
                    result,
                }
            });
        }
    }

    pub(super) fn due(&self) -> Option<Instant> {
        if self.reads.len() >= MAX_READS {
            return None;
        }

        self.entries
            .values()
            .filter(|entry| entry.dirty && !entry.running)
            .map(|entry| entry.not_before)
            .min()
    }

    pub(super) async fn next(&mut self) -> Result<Option<Reply>> {
        let Some(result) = self.reads.join_next().await else {
            return std::future::pending().await;
        };
        let Completed {
            generation,
            key,
            result,
        } = match result {
            Ok(completed) => completed,
            Err(error) if error.is_cancelled() => return Ok(None),
            Err(error) => return Err(error.into()),
        };

        if generation != self.generation {
            return Ok(None);
        }

        let retained: usize = self.entries.values().map(|entry| entry.bytes).sum();
        let Some(entry) = self.entries.get_mut(&key) else {
            return Ok(None);
        };
        entry.running = false;
        let touched = std::mem::take(&mut entry.reading);
        Ok(Some(match result {
            Ok((value, bytes)) => {
                if entry.previous.as_ref() == Some(&value) {
                    entry.idle = entry.idle.saturating_add(1);
                    // Folder listeners use the revision to recheck open files. An
                    // edit can leave every entry in the listing unchanged.
                    let folder_event = key.1.is_none()
                        && matches!(&entry.source, Projected::Read(Read::Sessions(request))
                            if matches!(request.as_ref(), SessionsRequest::Folder { .. }));

                    if !folder_event {
                        return Ok(None);
                    }
                } else {
                    entry.idle = 0;
                }

                if retained.saturating_sub(entry.bytes).saturating_add(bytes) > MAX_RETAINED_BYTES {
                    entry.previous = None;
                    entry.bytes = 0;
                    entry.sent = 0;
                    return Ok(Some(Reply::SnapshotError {
                        query: key.1,
                        topic: key.0.clone(),
                        reason: "snapshot cache budget exceeded; narrow the subscription".into(),
                    }));
                }

                let after = entry.sent;
                let delta = entry
                    .previous
                    .as_ref()
                    .filter(|_| after != 0)
                    .and_then(|before| {
                        if self.deltas && lists_sessions(&key) {
                            delta_of(before, &value, after)
                        } else if self.query_deltas && lists_external(&key) {
                            external_delta_of(before, &value, after)
                        } else {
                            None
                        }
                    });
                entry.bytes = bytes;
                entry.previous = Some(value.clone());
                self.revision = self
                    .revision
                    .checked_add(1)
                    .context("snapshot revision exhausted")?;
                entry.sent = self.revision;
                let value = named(delta.unwrap_or(value), touched);
                let sent = match &value {
                    Value::SessionsDelta(_) | Value::ExternalDelta(_) => {
                        rmp_serde::to_vec(&value)?.len()
                    }
                    _ => bytes,
                };
                let topic = key.0.name().to_owned();
                metrics::counter!("beep_snapshot_sent_bytes_total", "topic" => topic.clone())
                    .increment(sent as u64);
                metrics::histogram!("beep_snapshot_bytes", "topic" => topic).record(sent as f64);

                Reply::Snapshot(Box::new(Snapshot {
                    topic: key.0.clone(),
                    query: key.1,
                    revision: self.revision,
                    value,
                }))
            }
            Err(error) => {
                entry.previous = None;
                entry.bytes = 0;
                entry.sent = 0;
                entry.touched = Touched::All;
                entry.idle = entry.idle.saturating_add(1);
                Reply::SnapshotError {
                    query: key.1,
                    topic: key.0.clone(),
                    reason: format!("{error:#}"),
                }
            }
        }))
    }
}

/// `value`, a folder's listing, with the paths that changed since the one before it.
fn named(value: Value, touched: Touched) -> Value {
    match value {
        Value::Sessions(mut reply) => {
            if let SessionsReply::Tree { changed, .. } = reply.as_mut() {
                *changed = touched.into_list();
            }

            Value::Sessions(reply)
        }
        other => other,
    }
}

/// The names `replace` gives a source of their own. An exact topic of any other name is a
/// plugin's, and a name missing here would be overwritten by `Projected::Plugin`.
const PROJECTED: [&str; 18] = [
    "files",
    "agents",
    "agent",
    "agent-log",
    "processes",
    "process-output",
    "terminals",
    "terminal-output",
    "sync",
    "moves",
    MOVE_PROGRESS_TOPIC,
    "reviews",
    "sign-in",
    crate::run::device::LIST_TOPIC,
    crate::machine::provision::UPDATES_TOPIC,
    TEAM_CARDS_TOPIC,
    TEAM_INVITES_TOPIC,
    FORGE_REACH_TOPIC,
];

/// The topics called `name`.
fn topics_named<'a>(topics: &'a [Topic], name: &'a str) -> impl Iterator<Item = &'a Topic> {
    topics.iter().filter(move |topic| topic.name() == name)
}

/// The task `topic` names, when it is `name` keyed by a task number.
fn numbered(topic: &Topic, name: &str) -> Option<TaskId> {
    if topic.name() != name || topic.is_pattern() {
        return None;
    }

    topic.key()?.parse().ok().map(TaskId)
}

/// Whether the projection under `key` is the session list that deltas and rows apply to.
fn lists_sessions(key: &Key) -> bool {
    key.1.is_none() && key.0.name() == "agents"
}

/// Whether the projection under `key` is an external-session query that query deltas apply to.
fn lists_external(key: &Key) -> bool {
    key.1.is_some() && key.0.name() == "agents"
}

/// What the projection under `key` sends of `value`: the `agents` list holds
/// the rows a list shows (see `SessionInfo::row`) and nothing else.
fn shown(key: &Key, value: Value) -> Value {
    if !lists_sessions(key) {
        return value;
    }

    match value {
        Value::Sessions(reply) => Value::Sessions(Box::new(match *reply {
            SessionsReply::Sessions { sessions } => SessionsReply::Sessions {
                sessions: sessions.iter().map(SessionInfo::row).collect(),
            },
            other => other,
        })),
        other => other,
    }
}

/// What takes the session list `before` to `now`, when that is less to send
/// than `now`.
fn delta_of(before: &Value, now: &Value, after: u64) -> Option<Value> {
    let (Value::Sessions(before), Value::Sessions(now)) = (before, now) else {
        return None;
    };
    let (SessionsReply::Sessions { sessions: before }, SessionsReply::Sessions { sessions: now }) =
        (before.as_ref(), now.as_ref())
    else {
        return None;
    };
    let delta = SessionsDelta::between(before, now, after);

    (delta.changed.len() < now.len()).then(|| Value::SessionsDelta(Box::new(delta)))
}

/// What takes the external session list `before` to `now`, when that is less
/// to send than `now`.
fn external_delta_of(before: &Value, now: &Value, after: u64) -> Option<Value> {
    let (Value::Sessions(before), Value::Sessions(now)) = (before, now) else {
        return None;
    };
    let (SessionsReply::External { sessions: before }, SessionsReply::External { sessions: now }) =
        (before.as_ref(), now.as_ref())
    else {
        return None;
    };
    let delta = ExternalDelta::between(before, now, after);

    (delta.changed.len() < now.len()).then(|| Value::ExternalDelta(Box::new(delta)))
}

fn is_unkeyed_agent(topic: &Topic) -> bool {
    topic.name() == "agent" && topic.is_pattern() && topic.resource_segments().is_empty()
}

fn affected_entry(read: &Topic, source: &Projected, change: &Topic) -> bool {
    // External log writes emit unkeyed `agent` for internal log watchers
    // (spend, search, claude) and harness-scoped `agent-log.<agent>` for
    // external-session queries. Do not dirty unrelated session projections on
    // every line written to a harness log file.
    if is_unkeyed_agent(change) {
        return false;
    }

    if change.name() == "agent-log" {
        return match source {
            Projected::Read(Read::Sessions(request)) => match request.as_ref() {
                SessionsRequest::External { agent, .. } => {
                    change.is_pattern() || change.key() == Some(agent.as_str())
                }
                SessionsRequest::InFolders { .. } => true,
                _ => false,
            },
            _ => false,
        };
    }

    affected(read, change)
}

fn affected(read: &Topic, change: &Topic) -> bool {
    read.matches(change)
        || matches!(
            (read.name(), change.name()),
            ("agents", "agent" | "moves")
                | ("agent", "agents")
                | ("processes", "process-output")
                | ("process-output", "processes")
                | ("terminal-output", "terminals")
                | (crate::run::device::LIST_TOPIC, crate::run::device::TOPIC)
        )
}

async fn read_value(offers: Offers, caller: iroh::EndpointId, read: Read) -> Result<Value> {
    match read {
        Read::Git { root, query } => super::git::read(&offers.agents.reach, &root, query)
            .await
            .map(Value::Git),
        Read::Sessions(request) => {
            match execute(offers.agents, caller, Operation::Sessions(*request)).await? {
                CallReply::Sessions(reply) => Ok(Value::Sessions(reply)),
                _ => bail!("unexpected snapshot reply"),
            }
        }
        Read::SignIn { tool } => {
            let state = ask(
                &beep_proto::node::sign_in::Request::State { tool },
                crate::machine::sign_in::serve,
            )
            .await?;
            Ok(Value::SignIn(
                state.context("sign-in state closed without a reply")?,
            ))
        }
        Read::Process(request) => Ok(Value::Process(
            crate::run::tasks::answer(crate::run::tasks::Table::Shared, request).await,
        )),
        Read::TerminalList => {
            let list = ask(
                &beep_proto::node::terminal::Request::List {},
                crate::run::terminal::serve,
            )
            .await?;
            Ok(Value::Terminals(
                list.context("terminal list closed without state")?,
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
#[path = "../../tests/link_events_snapshots.rs"]
mod tests;
