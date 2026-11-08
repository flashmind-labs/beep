//! Pushes an initial snapshot and subsequent changes to an open session view.

use super::{Agents, MAX_TRANSCRIPT, found::Agent};
use anyhow::{Context, Result, bail};
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::{FollowSession, SessionSnapshot, SessionSource, SessionUpdate, SessionsReply},
};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{future::pending, path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    sync::{Semaphore, mpsc},
    task::spawn_blocking,
    time::{Instant, MissedTickBehavior, interval_at, sleep, timeout},
};

static READERS: Semaphore = Semaphore::const_new(32);
const HEARTBEAT: Duration = Duration::from_secs(10);
const WRITE_WAIT: Duration = Duration::from_secs(15);

pub(super) struct Sources {
    _watcher: RecommendedWatcher,
    _summary: tokio_util::task::AbortOnDropHandle<()>,
}

impl Agents {
    /// One filesystem observer per node, shared by every topic subscriber.
    pub(crate) async fn watch_event_sources(self: &Arc<Self>) -> Result<()> {
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
        let agents = self.clone();
        spawn_blocking(move || {
            let mut held = crate::lock(&agents.event_watcher);
            if held.is_some() {
                return Ok(());
            }
            let mut roots: Vec<_> = [Agent::Codex, Agent::Pi, Agent::Flsh, Agent::OpenCode]
                .into_iter()
                .flat_map(|agent| agents.stores.follow_paths(Some(agent)))
                .chain(agents.external.follow_root())
                .filter(|root| root.exists())
                .collect();
            roots.sort();
            roots.dedup();
            let paths = roots.clone();
            let hub = agents.events.clone();
            let (dirty, mut summaries) = mpsc::channel(1);
            let summary_hub = hub.clone();
            let summary = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
                while summaries.recv().await.is_some() {
                    sleep(Duration::from_millis(400)).await;
                    while summaries.try_recv().is_ok() {}
                    summary_hub.changed(beep_proto::node::events::Topic::new("agents"));
                }
            }));
            let mut watcher = RecommendedWatcher::new(
                move |event: notify::Result<notify::Event>| {
                    if event.as_ref().is_ok_and(|event| {
                        matches!(event.kind, EventKind::Access(_) | EventKind::Other)
                            || !event
                                .paths
                                .iter()
                                .any(|path| paths.iter().any(|root| path.starts_with(root)))
                    }) {
                        return;
                    }
                    hub.changed(beep_proto::node::events::Topic::new("agent"));
                    let _ = dirty.try_send(());
                },
                notify::Config::default(),
            )?;
            for root in roots {
                watcher.watch(&root, RecursiveMode::Recursive)?;
            }
            *held = Some(Sources {
                _watcher: watcher,
                _summary: summary,
            });
            Ok(())
        })
        .await
        .context("starting session notifications")?
    }
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
    let mut watcher = RecommendedWatcher::new(
        move |event: notify::Result<notify::Event>| {
            if event.as_ref().map_or(true, |event| {
                matches!(
                    event.kind,
                    EventKind::Create(_)
                        | EventKind::Modify(_)
                        | EventKind::Remove(_)
                        | EventKind::Any
                )
            }) {
                let _ = dirty.try_send(());
            }
        },
        notify::Config::default(),
    )?;
    for path in paths {
        watcher.watch(&path, RecursiveMode::Recursive)?;
    }
    let mut heartbeat = interval_at(Instant::now() + HEARTBEAT, HEARTBEAT);
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut previous = None;
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
        if previous.as_ref() != Some(&snapshot) {
            let update = since(&snapshot, previous.as_ref());
            send(tx, &SessionUpdate::Snapshot(Box::new(update))).await?;
            previous = Some(snapshot);
        }
        tokio::select! {
            changed = async {
                match &mut live {
                    Some(live) => live.changed().await,
                    None => pending().await,
                }
            } => if changed.is_err() { return Ok(()); },
            changed = presence.changed() => if changed.is_err() { return Ok(()); },
            changed = changes.recv() => if changed.is_none() { return Ok(()); },
            _ = heartbeat.tick() => {
                send(tx, &SessionUpdate::KeepAlive).await?;
                // Reconcile process exit and missed filesystem notifications locally.
            },
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
    if request.agent == "claude" {
        return Ok(agents
            .external
            .file(&request.session)
            .await?
            .and_then(|file| file.parent().map(PathBuf::from))
            .into_iter()
            .collect());
    }
    let stores = agents.stores.clone();
    let agent = Agent::from_name(&request.agent);
    spawn_blocking(move || stores.follow_paths(agent))
        .await
        .context("finding session files")
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::node::agent::{Entry, EntryKind};

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
