//! Pushes an initial snapshot and subsequent changes to an open session view.

use super::{Agents, MAX_TRANSCRIPT, found::Agent};
use anyhow::{Context, Result, bail};
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::{FollowSession, SessionSnapshot, SessionSource, SessionUpdate, SessionsReply},
};
use std::{future::pending, path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    sync::{Semaphore, mpsc},
    task::spawn_blocking,
    time::{Instant, MissedTickBehavior, interval_at, sleep, timeout},
};

static READERS: Semaphore = Semaphore::const_new(32);
const HEARTBEAT: Duration = Duration::from_secs(10);
const WRITE_WAIT: Duration = Duration::from_secs(15);

pub(crate) struct Sources {
    paths: crate::events::paths::Paths,
    processes: std::sync::Mutex<std::collections::HashMap<(String, String), ProcessWatch>>,
    _summary: tokio_util::task::AbortOnDropHandle<()>,
}

#[derive(PartialEq, Eq)]
enum Observation {
    Process(u32),
    Recent(u64),
}

struct ProcessWatch {
    revision: Observation,
    seen: Instant,
    cwd: PathBuf,
    _task: tokio_util::task::AbortOnDropHandle<()>,
}

fn active_roots(
    processes: &std::collections::HashMap<(String, String), ProcessWatch>,
) -> Vec<PathBuf> {
    let mut roots: Vec<_> = processes
        .values()
        .filter(|held| {
            matches!(held.revision, Observation::Process(_)) && !held._task.is_finished()
        })
        .map(|held| held.cwd.clone())
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

impl Agents {
    pub(super) fn observe_external(&self, sessions: &[beep_proto::node::agent::ExternalSession]) {
        let Some(source) = crate::lock(&self.event_watcher).upgrade() else {
            return;
        };
        let mut processes = crate::lock(&source.processes);
        let before = active_roots(&processes);
        processes.retain(|_, held| !held._task.is_finished());
        for session in sessions {
            let key = (session.agent.clone(), session.id.clone());
            let revision = match (session.pid, recent_deadline(session)) {
                (Some(pid), _) => Observation::Process(pid),
                (_, Some(_)) => Observation::Recent(session.last_activity),
                _ => {
                    processes.remove(&key);
                    continue;
                }
            };
            if let Some(held) = processes.get_mut(&key)
                && held.revision == revision
            {
                held.seen = Instant::now();
                held.cwd = PathBuf::from(&session.cwd);
                continue;
            }
            if processes.len() >= 512
                && !processes.contains_key(&key)
                && let Some(oldest) = processes
                    .iter()
                    .min_by_key(|(_, held)| held.seen)
                    .map(|(key, _)| key.clone())
            {
                processes.remove(&oldest);
            }
            let exit = match revision {
                Observation::Process(pid) => match crate::events::exits::Exit::watch(pid) {
                    Ok(exit) => Some(exit),
                    Err(error) => {
                        log::debug!("external process exit cannot be observed: {error}");
                        continue;
                    }
                },
                Observation::Recent(_) => None,
            };
            let deadline = recent_deadline(session);
            let hub = self.events.clone();
            let id = session.id.clone();
            let task = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
                match exit {
                    Some(exit) => exit.wait().await,
                    None => {
                        if let Some(deadline) = deadline {
                            tokio::time::sleep_until(deadline).await;
                        }
                    }
                }
                hub.changed(beep_proto::node::events::Topic::keyed("agent", &id));
                hub.changed(beep_proto::node::events::Topic::new("agents"));
            }));
            processes.insert(
                key,
                ProcessWatch {
                    revision,
                    seen: Instant::now(),
                    cwd: PathBuf::from(&session.cwd),
                    _task: task,
                },
            );
        }
        let changed = before != active_roots(&processes);
        drop(processes);
        if changed {
            self.events
                .changed(beep_proto::node::events::Topic::new("agents"));
        }
    }

    pub(super) fn observed_work_roots(&self) -> Vec<PathBuf> {
        crate::lock(&self.event_watcher)
            .upgrade()
            .map_or_else(Vec::new, |source| {
                active_roots(&crate::lock(&source.processes))
            })
    }

    pub(crate) fn start_event_bridge(&self) {
        self.event_forward.get_or_init(|| {
            let mut source = crate::events::system().subscribe();
            let hub = self.events.clone();
            tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
                loop {
                    match source.recv().await {
                        Ok(event) => hub.changed(event.topic),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            hub.changed(beep_proto::node::events::Topic::new("*"))
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                    }
                }
            }))
        });
    }

    /// Log observers exist only while some client follows agent topics.
    pub(crate) async fn watch_event_sources(self: &Arc<Self>) -> Result<Arc<Sources>> {
        if let Some(held) = crate::lock(&self.event_watcher).upgrade() {
            return Ok(held);
        }
        let roots: Vec<_> = [Agent::Codex, Agent::Pi, Agent::Flsh, Agent::OpenCode]
            .into_iter()
            .flat_map(|agent| self.stores.follow_paths(Some(agent)))
            .chain(self.external.follow_paths())
            .collect();
        let hub = self.events.clone();
        let agents = Arc::downgrade(self);
        let source = spawn_blocking(move || {
            let (dirty, mut summaries) = mpsc::channel(1);
            let structure = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let summary_structure = structure.clone();
            let summary_hub = hub.clone();
            let summary = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
                let mut previous = None;
                while summaries.recv().await.is_some() {
                    sleep(Duration::from_millis(400)).await;
                    while summaries.try_recv().is_ok() {}
                    let Some(agents) = agents.upgrade() else {
                        return;
                    };
                    if summary_structure.swap(false, std::sync::atomic::Ordering::AcqRel) {
                        *agents.sighted.lock().await = None;
                    }
                    match source_summary(&agents).await {
                        Ok(summary) if previous.as_ref() != Some(&summary) => {
                            previous = Some(summary);
                            summary_hub.changed(beep_proto::node::events::Topic::new("agents"));
                        }
                        Ok(_) => {}
                        Err(error) => {
                            log::debug!("agent summary could not be reconciled: {error}");
                            summary_hub.changed(beep_proto::node::events::Topic::new("agents"));
                        }
                    }
                }
            }));
            let paths = crate::events::paths::Paths::with_structure(move |_, structural| {
                if structural {
                    structure.store(true, std::sync::atomic::Ordering::Release);
                }
                hub.changed(beep_proto::node::events::Topic::new("agent"));
                let _ = dirty.try_send(());
            })?;
            Ok::<_, anyhow::Error>(Arc::new(Sources {
                paths,
                processes: std::sync::Mutex::default(),
                _summary: summary,
            }))
        })
        .await
        .context("starting session notifications")??;
        source.paths.replace(roots).await?;
        let mut held = crate::lock(&self.event_watcher);
        if let Some(current) = held.upgrade() {
            return Ok(current);
        }
        *held = Arc::downgrade(&source);
        Ok(source)
    }
}

#[derive(Debug, PartialEq)]
struct Summary {
    id: String,
    agent: String,
    title: Option<String>,
    cwd: String,
    presence: beep_proto::node::agent::Presence,
    pid: Option<u32>,
}

async fn source_summary(agents: &Agents) -> Result<Vec<Summary>> {
    let stores = agents.stores.clone();
    let described = spawn_blocking(move || {
        let mut seen = stores.sessions(None);
        seen.sort_by_key(|session| std::cmp::Reverse(session.last_activity));
        seen.truncate(512);
        let now = std::time::SystemTime::now();
        seen.into_iter()
            .map(|session| stores.describe(session, now))
            .collect::<Vec<_>>()
    });
    let (claude, others) = tokio::join!(agents.external.list(512, None, agents.owned()), described);
    let mut sessions = claude?;
    agents.observe_external(&sessions);
    sessions.extend(others.context("reading agent summary")?);
    agents.presence.merge(&mut sessions);
    let mut summary: Vec<_> = sessions
        .into_iter()
        .map(|session| Summary {
            id: session.id,
            agent: session.agent,
            title: session.title,
            cwd: session.cwd,
            presence: session.presence,
            pid: session.pid,
        })
        .collect();
    summary.sort_by(|a, b| (&a.agent, &a.id).cmp(&(&b.agent, &b.id)));
    Ok(summary)
}

pub async fn serve_follow(mut tx: LinkTx, mut rx: LinkRx, agents: Arc<Agents>) -> Result<()> {
    let request: FollowSession = timeout(WRITE_WAIT, rx.recv())
        .await??
        .context("session subscription closed before its request")?;
    let Ok(_reader) = READERS.try_acquire() else {
        tx.send(&SessionUpdate::Error {
            reason: "too many session readers".into(),
        })
        .await?;
        return Ok(());
    };
    // Dropping the reader cancels reads and writes too, including a blocked write.
    tokio::select! {
        result = follow(&mut tx, &agents, request) => result,
        _ = rx.recv::<serde_json::Value>() => Ok(()),
    }
}

async fn send(tx: &mut LinkTx, update: &SessionUpdate) -> Result<()> {
    timeout(WRITE_WAIT, tx.send(update))
        .await
        .context("session reader stopped receiving")?
}

async fn follow(tx: &mut LinkTx, agents: &Agents, request: FollowSession) -> Result<()> {
    let mut presence = agents.presence.subscribe();
    let mut live = agents
        .get(&request.session)
        .map(|session| session.live.clone());
    // Install file notifications before the initial read so a write cannot fall between them.
    let paths = paths(agents, &request).await?;
    let (dirty, mut changes) = mpsc::channel(1);
    let _watcher = if paths.is_empty() {
        None
    } else {
        let watcher = crate::events::paths::Paths::new(move |_| {
            let _ = dirty.try_send(());
        })?;
        watcher.replace(paths).await?;
        Some(watcher)
    };
    let mut heartbeat = interval_at(Instant::now() + HEARTBEAT, HEARTBEAT);
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut previous = None;
    let mut observed_pid = None;
    let mut exit = None;
    loop {
        presence.borrow_and_update();
        if let Some(live) = &mut live {
            live.borrow_and_update();
        }
        let snapshot = match snapshot(agents, &request).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return send(
                    tx,
                    &SessionUpdate::Error {
                        reason: format!("{error:#}"),
                    },
                )
                .await;
            }
        };
        let pid = match &snapshot.source {
            SessionSource::External(session) => session.pid,
            SessionSource::Owned(_) => None,
        };
        if pid != observed_pid {
            exit = pid.and_then(|pid| match crate::events::exits::Exit::watch(pid) {
                Ok(exit) => Some(exit),
                Err(error) => {
                    log::debug!("session process exit cannot be observed: {error}");
                    None
                }
            });
            observed_pid = pid;
        }
        if previous.as_ref() != Some(&snapshot) {
            let update = since(&snapshot, previous.as_ref());
            send(tx, &SessionUpdate::Snapshot(Box::new(update))).await?;
            previous = Some(snapshot);
        }
        let follow_presence = live.is_none();
        loop {
            tokio::select! {
                changed = async {
                    match &mut live {
                        Some(live) => live.changed().await,
                        None => pending().await,
                    }
                } => if changed.is_err() { return Ok(()); },
                changed = async {
                    if follow_presence { presence.changed().await } else { pending().await }
                } => if changed.is_err() { return Ok(()); },
                changed = changes.recv() => if changed.is_none() { return Ok(()); },
                _ = async {
                    match &exit {
                        Some(exit) => exit.wait().await,
                        None => pending().await,
                    }
                } => { exit = None; },
                _ = async {
                    match &previous {
                        Some(snapshot) => match &snapshot.source {
                            SessionSource::External(session) => match recent_deadline(session) {
                                Some(deadline) => tokio::time::sleep_until(deadline).await,
                                None => pending().await,
                            },
                            _ => pending().await,
                        },
                        None => pending().await,
                    }
                } => {},
                _ = heartbeat.tick() => {
                    send(tx, &SessionUpdate::KeepAlive).await?;
                    continue;
                },
            }
            break;
        }
        // Coalesce token bursts; the watch channel and file queue retain a dirty bit.
        sleep(Duration::from_millis(40)).await;
        if live.is_none() {
            live = agents
                .get(&request.session)
                .map(|session| session.live.clone());
        }
    }
}

// Both log readers classify an unowned write as recent for three minutes.
// Schedule that single state transition, rather than periodically rereading idle logs.
fn recent_deadline(session: &beep_proto::node::agent::ExternalSession) -> Option<Instant> {
    if session.pid.is_some() || session.presence != beep_proto::node::agent::Presence::Recent {
        return None;
    }
    let remaining = session
        .last_activity
        .saturating_add(181)
        .saturating_sub(super::unix_now());
    Instant::now().checked_add(Duration::from_secs(remaining.max(1)))
}

/// Keeps only the suffix that replaces entries the reader already holds.
fn since(snapshot: &SessionSnapshot, previous: Option<&SessionSnapshot>) -> SessionSnapshot {
    let mut update = snapshot.clone();
    let Some(previous) = previous else {
        return update;
    };
    let Some(offset) = snapshot
        .earlier
        .checked_sub(previous.earlier)
        .and_then(|offset| usize::try_from(offset).ok())
    else {
        return update;
    };
    if offset > previous.entries.len() {
        return update;
    }
    let same = snapshot
        .entries
        .iter()
        .zip(previous.entries.iter().skip(offset))
        .take_while(|(new, old)| new == old)
        .count();
    update.entries.drain(..same);
    update.earlier += same as u64;
    update
}

async fn snapshot(agents: &Agents, request: &FollowSession) -> Result<SessionSnapshot> {
    let source = match agents.get(&request.session) {
        Some(session) => {
            let info = session.live.borrow().info.clone();
            if info.agent != request.agent {
                bail!("session belongs to another agent");
            }
            SessionSource::Owned(Box::new(info))
        }
        None => {
            let found = if request.agent == "claude" {
                agents
                    .external
                    .list(1, Some(request.session.clone()), agents.owned())
                    .await?
            } else {
                agents
                    .stores_external(&request.agent, 1, Some(request.session.clone()))
                    .await?
            };
            let found = found
                .into_iter()
                .find(|session| session.id == request.session)
                .context("session is no longer on the machine")?;
            SessionSource::External(Box::new(found))
        }
    };
    let last = request.last.clamp(1, MAX_TRANSCRIPT);
    match agents
        .transcript_reply(&request.session, last, true, None)
        .await
    {
        SessionsReply::Transcript { entries, earlier } => Ok(SessionSnapshot {
            source,
            entries,
            earlier: u64::from(earlier),
        }),
        SessionsReply::Error { message } => bail!(message),
        _ => bail!("unexpected transcript reply"),
    }
}

async fn paths(agents: &Agents, request: &FollowSession) -> Result<Vec<PathBuf>> {
    if agents.get(&request.session).is_some() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    if let Some(file) = agents.session_file(&request.session).await? {
        if matches!(request.agent.as_str(), "flsh" | "opencode") {
            let mut wal = file.as_os_str().to_owned();
            wal.push("-wal");
            paths.push(PathBuf::from(wal));
        }
        paths.push(file);
    }
    if request.agent == "claude" {
        paths.extend(agents.external.presence_path());
    } else if request.agent == "codex" {
        paths.extend(
            agents
                .stores
                .follow_paths(Some(Agent::Codex))
                .into_iter()
                .filter(|path| {
                    path.file_name()
                        .is_some_and(|name| name == "session_index.jsonl")
                }),
        );
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::node::agent::{Entry, EntryKind};

    #[tokio::test]
    async fn event_sources_are_shared_and_stop_with_the_last_subscription() {
        let root = tempfile::tempdir().unwrap();
        let agents = Arc::new(Agents {
            stores: super::super::found::Stores::at(Some(root.path()), None, None),
            external: super::super::claude::External::locate_in(root.path()),
            ..Agents::default()
        });
        let first = agents.watch_event_sources().await.unwrap();
        let second = agents.watch_event_sources().await.unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        let weak = Arc::downgrade(&first);
        drop(first);
        assert!(weak.upgrade().is_some());
        drop(second);
        assert!(weak.upgrade().is_none());
        let restarted = agents.watch_event_sources().await.unwrap();
        assert!(!std::sync::Weak::ptr_eq(&weak, &Arc::downgrade(&restarted)));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_external_process_exit_publishes_agent_and_summary_events() {
        let root = tempfile::tempdir().unwrap();
        let agents = Arc::new(Agents {
            stores: super::super::found::Stores::at(Some(root.path()), None, None),
            external: super::super::claude::External::locate_in(root.path()),
            ..Agents::default()
        });
        let _lease = agents.watch_event_sources().await.unwrap();
        let mut changes = agents.events.subscribe();
        let mut child = tokio::process::Command::new("sh")
            .args(["-c", "read message"])
            .stdin(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        agents.observe_external(&[beep_proto::node::agent::ExternalSession {
            id: "external-example".into(),
            agent: "codex".into(),
            cwd: root.path().to_string_lossy().into_owned(),
            pid: child.id(),
            ..Default::default()
        }]);
        assert_eq!(agents.observed_work_roots(), [root.path().to_owned()]);
        drop(child.stdin.take());
        timeout(Duration::from_secs(5), async {
            loop {
                let event = changes.recv().await.unwrap();
                if event.topic
                    == beep_proto::node::events::Topic::keyed("agent", "external-example")
                {
                    break;
                }
            }
            loop {
                let event = changes.recv().await.unwrap();
                if event.topic.name() == "agents" {
                    break;
                }
            }
        })
        .await
        .unwrap();
        child.wait().await.unwrap();
        assert!(agents.observed_work_roots().is_empty());
    }

    #[tokio::test]
    async fn source_summaries_ignore_transcript_appends_but_notice_title_changes() {
        use std::{fs::OpenOptions, io::Write};
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions/2026/11/01");
        std::fs::create_dir_all(&sessions).unwrap();
        let file = sessions.join("rollout-example.jsonl");
        std::fs::write(&file, "{\"type\":\"session_meta\",\"payload\":{\"id\":\"example\",\"cwd\":\"/srv/project\",\"source\":\"cli\"}}\n").unwrap();
        let agents = Agents {
            stores: super::super::found::Stores::at(Some(root.path()), None, None),
            ..Agents::default()
        };
        let before = source_summary(&agents).await.unwrap();
        assert_eq!(before.len(), 1);
        let mut writer = OpenOptions::new().append(true).open(file).unwrap();
        writeln!(writer, "{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\",\"message\":\"output\"}}}}").unwrap();
        assert_eq!(source_summary(&agents).await.unwrap(), before);
        std::fs::write(
            root.path().join("session_index.jsonl"),
            "{\"id\":\"example\",\"thread_name\":\"Renamed session\"}\n",
        )
        .unwrap();
        assert_eq!(
            source_summary(&agents).await.unwrap()[0].title.as_deref(),
            Some("Renamed session")
        );
    }

    #[tokio::test]
    async fn database_only_sessions_return_the_existing_transcript_error_and_refresh_metadata() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("project");
        std::fs::create_dir(&folder).unwrap();
        let database = rusqlite::Connection::open(folder.join("sessions.db")).unwrap();
        database.execute_batch("PRAGMA journal_mode = WAL;
            CREATE TABLE session_meta (chat_key TEXT PRIMARY KEY, title TEXT, working_dir TEXT, created_at INTEGER);
            CREATE TABLE sessions (chat_key TEXT, entry_kind TEXT, content TEXT, turn_index INTEGER, created_at INTEGER);
            INSERT INTO session_meta VALUES ('example', 'Original', '/srv/project', 100);").unwrap();
        let agents = Arc::new(Agents {
            stores: super::super::found::Stores::at(None, None, Some(root.path())),
            ..Agents::default()
        });
        assert_eq!(
            source_summary(&agents).await.unwrap()[0].title.as_deref(),
            Some("Original")
        );
        let link = beep_proto::node::local_link();
        let task = tokio::spawn(serve_follow(link.node.0, link.node.1, agents.clone()));
        let (mut tx, mut rx) = link.caller;
        tx.send(&FollowSession {
            agent: "flsh".into(),
            session: "example".into(),
            last: 200,
        })
        .await
        .unwrap();
        let update = timeout(Duration::from_secs(5), rx.recv::<SessionUpdate>())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(
            matches!(update, SessionUpdate::Error { reason } if reason.contains("flsh's transcript cannot be read yet"))
        );
        task.await.unwrap().unwrap();
        database
            .execute("UPDATE session_meta SET title = 'Changed'", [])
            .unwrap();
        assert_eq!(
            source_summary(&agents).await.unwrap()[0].title.as_deref(),
            Some("Changed")
        );
    }

    fn page(earlier: u64, text: &[&str]) -> SessionSnapshot {
        SessionSnapshot {
            source: SessionSource::Owned(Box::default()),
            entries: text
                .iter()
                .map(|text| Entry::new(EntryKind::Agent, (*text).into()))
                .collect(),
            earlier,
        }
    }

    #[test]
    fn updates_replace_a_changed_entry_and_send_only_the_changed_suffix() {
        let before = page(3, &["one", "thi"]);
        let after = page(3, &["one", "thinking", "done"]);
        assert_eq!(since(&after, Some(&before)), page(4, &["thinking", "done"]));
        assert_eq!(since(&after, None), after);
        assert_eq!(since(&after, Some(&after)), page(6, &[]));
    }

    #[test]
    fn a_moving_window_keeps_indices_and_a_gap_or_truncation_resynchronizes() {
        let before = page(3, &["one", "two", "three"]);
        assert_eq!(
            since(&page(4, &["two", "three", "four"]), Some(&before)),
            page(6, &["four"])
        );
        assert_eq!(
            since(&page(8, &["later"]), Some(&before)),
            page(8, &["later"])
        );
        assert_eq!(
            since(&page(0, &["reset"]), Some(&before)),
            page(0, &["reset"])
        );
        assert_eq!(since(&page(3, &["one"]), Some(&before)), page(4, &[]));
    }

    #[tokio::test]
    async fn an_external_transcript_pushes_appends_and_reconnects_with_a_full_snapshot() {
        use super::super::claude::External;
        use std::{fs::OpenOptions, io::Write};
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("projects/project");
        std::fs::create_dir_all(&folder).unwrap();
        let id = "0f3c2a9e-1111-4222-8333-944455556666";
        let file = folder.join(format!("{id}.jsonl"));
        std::fs::write(&file, "{\"type\":\"user\",\"cwd\":\"/srv/project\",\"timestamp\":\"2026-10-29T08:00:00Z\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n").unwrap();
        let agents = Arc::new(Agents {
            external: External::locate_in(root.path()),
            ..Agents::default()
        });
        let request = FollowSession {
            agent: "claude".into(),
            session: id.into(),
            last: 200,
        };
        async fn connect(
            agents: Arc<Agents>,
            request: &FollowSession,
        ) -> (LinkTx, LinkRx, tokio::task::JoinHandle<Result<()>>) {
            let link = beep_proto::node::local_link();
            let task = tokio::spawn(serve_follow(link.node.0, link.node.1, agents));
            let (mut tx, rx) = link.caller;
            tx.send(request).await.unwrap();
            (tx, rx, task)
        }
        async fn next(rx: &mut LinkRx) -> SessionSnapshot {
            match timeout(Duration::from_secs(5), rx.recv::<SessionUpdate>())
                .await
                .unwrap()
                .unwrap()
                .unwrap()
            {
                SessionUpdate::Snapshot(snapshot) => *snapshot,
                other => panic!("expected a snapshot, got {other:?}"),
            }
        }
        let (tx, mut rx, task) = connect(agents.clone(), &request).await;
        assert_eq!(next(&mut rx).await.entries[0].text, "hello");
        let mut writer = OpenOptions::new().append(true).open(&file).unwrap();
        writeln!(writer, "{{\"type\":\"assistant\",\"timestamp\":\"2026-10-29T08:00:01Z\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"text\",\"text\":\"world\"}}]}}}}").unwrap();
        let update = next(&mut rx).await;
        assert_eq!(update.earlier, 1);
        assert_eq!(update.entries[0].text, "world");
        drop(tx);
        timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let (tx, mut rx, task) = connect(agents, &request).await;
        let snapshot = next(&mut rx).await;
        assert_eq!(snapshot.earlier, 0);
        assert_eq!(snapshot.entries.len(), 2);
        drop(tx);
        timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}
