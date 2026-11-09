//! Pushes an initial snapshot and subsequent changes to an open session view.

use super::{Agents, MAX_TRANSCRIPT, found::Agent};
use anyhow::{Context, Result, bail};
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::{Entry, FollowSession, SessionCursor, SessionSnapshot, SessionSource, SessionUpdate},
};
use std::{future::pending, path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    sync::{Semaphore, mpsc},
    task::spawn_blocking,
    time::{Instant, MissedTickBehavior, interval_at, sleep, timeout},
};

static READERS: Semaphore = Semaphore::const_new(32);
const RETAINED_ENTRIES: usize = 5000;
// Leave room for source metadata inside the transport's 8 MiB frame limit.
const RETAINED_BYTES: usize = 4 * 1024 * 1024;

const HEARTBEAT: Duration = Duration::from_secs(10);
const WRITE_WAIT: Duration = Duration::from_secs(15);

mod sources;
pub(crate) use sources::Sources;

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
        .get(request.session.as_ref())
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

        let prior = previous.take();
        let requested = request.clone();
        let (snapshot, update, changed) = spawn_blocking(move || {
            let (snapshot, update) = next_update(snapshot, prior.as_ref(), &requested)?;
            let changed = prior.as_ref().is_none_or(|prior| {
                prior.cursor != snapshot.cursor || prior.source != snapshot.source
            });
            Ok::<_, anyhow::Error>((snapshot, update, changed))
        })
        .await
        .context("preparing transcript update")??;
        previous = Some(snapshot);

        if changed {
            send(tx, &update).await?;
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
                .get(request.session.as_ref())
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

/// Hash exactly the canonical window the client can retain and validate on reconnect.
fn fingerprint(start: u64, entries: &[Entry]) -> Result<String> {
    let wire = rmp_serde::to_vec(entries).context("encoding transcript checkpoint")?;
    let mut hash = blake3::Hasher::new();
    hash.update(b"session-window/1");
    hash.update(&start.to_le_bytes());
    hash.update(&u64::try_from(wire.len())?.to_le_bytes());
    hash.update(&wire);
    Ok(hash.finalize().to_hex().to_string())
}

fn window(mut snapshot: SessionSnapshot, start: u64) -> Result<SessionSnapshot> {
    let skip = start
        .saturating_sub(snapshot.earlier)
        .min(snapshot.entries.len() as u64) as usize;
    snapshot.entries.drain(..skip);
    snapshot.earlier += skip as u64;
    snapshot.cursor.start = snapshot.earlier;
    snapshot.cursor.fingerprint = fingerprint(snapshot.earlier, &snapshot.entries)?;
    Ok(snapshot)
}

fn validates(snapshot: &SessionSnapshot, cursor: &SessionCursor) -> Result<bool> {
    if cursor.start < snapshot.earlier
        || cursor.next < cursor.start
        || cursor.next > snapshot.cursor.next
    {
        return Ok(false);
    }

    let start = (cursor.start - snapshot.earlier) as usize;
    let end = (cursor.next - snapshot.earlier) as usize;
    Ok(fingerprint(cursor.start, &snapshot.entries[start..end])? == cursor.fingerprint)
}

/// Keeps only the suffix that replaces entries the reader already holds.
fn since(snapshot: &SessionSnapshot, previous: &SessionSnapshot) -> SessionSnapshot {
    let offset = (snapshot.earlier - previous.earlier) as usize;
    let same = if snapshot.cursor == previous.cursor {
        snapshot.entries.len()
    } else {
        snapshot
            .entries
            .iter()
            .zip(previous.entries.iter().skip(offset))
            .take_while(|(new, old)| new == old)
            .count()
    };
    suffix(snapshot, same, false)
}

fn suffix(snapshot: &SessionSnapshot, skip: usize, resumed: bool) -> SessionSnapshot {
    SessionSnapshot {
        source: snapshot.source.clone(),
        entries: snapshot.entries[skip..].to_vec(),
        earlier: snapshot.earlier + skip as u64,
        cursor: snapshot.cursor.clone(),
        resumed,
    }
}

fn retained(mut snapshot: SessionSnapshot) -> Result<SessionSnapshot> {
    // Reserve the largest compact array header before counting complete entries.
    let mut bytes = 5usize;
    let mut kept = 0;

    for entry in snapshot.entries.iter().rev() {
        let encoded = rmp_serde::to_vec(entry).context("measuring transcript entry")?;
        let next = bytes
            .checked_add(encoded.len())
            .context("transcript size overflow")?;
        if next > RETAINED_BYTES {
            break;
        }
        bytes = next;
        kept += 1;
    }

    if kept == 0 && !snapshot.entries.is_empty() {
        bail!("a transcript entry exceeds the retained window limit");
    }

    let removed = snapshot.entries.len() - kept;
    snapshot.entries.drain(..removed);
    snapshot.earlier = snapshot
        .earlier
        .checked_add(u64::try_from(removed)?)
        .context("transcript position overflow")?;
    snapshot.cursor.start = snapshot.earlier;
    Ok(snapshot)
}

fn next_update(
    snapshot: SessionSnapshot,
    previous: Option<&SessionSnapshot>,
    request: &FollowSession,
) -> Result<(SessionSnapshot, SessionUpdate)> {
    let snapshot = retained(snapshot)?;
    let initial_start = snapshot
        .cursor
        .next
        .saturating_sub(u64::from(request.last.clamp(1, MAX_TRANSCRIPT)));
    if let Some(previous) = previous {
        // Retention loss, a source reset, or truncation before our window requires a fresh view.
        if snapshot.earlier <= previous.cursor.next && previous.earlier < snapshot.cursor.next {
            let snapshot = window(snapshot, previous.earlier)?;
            let delta = since(&snapshot, previous);
            return Ok((snapshot, SessionUpdate::Delta(Box::new(delta))));
        }
    } else if let Some(after) = &request.after
        && validates(&snapshot, after)?
    {
        let snapshot = window(snapshot, after.start)?;
        let skip = (after.next - snapshot.earlier) as usize;
        let update = suffix(&snapshot, skip, true);
        return Ok((snapshot, SessionUpdate::Snapshot(Box::new(update))));
    }

    let snapshot = window(snapshot, initial_start)?;
    Ok((
        snapshot.clone(),
        SessionUpdate::Snapshot(Box::new(snapshot)),
    ))
}

async fn snapshot(agents: &Agents, request: &FollowSession) -> Result<SessionSnapshot> {
    let source = match agents.get(request.session.as_ref()) {
        Some(session) => {
            let info = session.live.borrow().info.clone();

            if info.agent != request.agent.as_ref() {
                bail!("session belongs to another agent");
            }

            SessionSource::Owned(Box::new(info))
        }
        None => {
            let found = if request.agent == "claude" {
                agents
                    .external
                    .list(1, Some(request.session.to_string()), agents.owned())
                    .await?
            } else {
                agents
                    .stores_external(request.agent.as_ref(), 1, Some(request.session.to_string()))
                    .await?
            };
            let found = found
                .into_iter()
                .find(|session| session.id == request.session.as_ref())
                .context("session is no longer on the machine")?;
            SessionSource::External(Box::new(found))
        }
    };
    // Source readers already bound their retained transcript and memoize disk parsing.
    // Read enough to validate the cached window, including mutable tools before its tail.
    let (entries, earlier) = if let Some(session) = agents.get(request.session.as_ref()) {
        agents
            .live_transcript(&session, RETAINED_ENTRIES, None)
            .await
    } else {
        let read = match agents
            .external
            .transcript(request.session.as_ref(), RETAINED_ENTRIES, None)
            .await?
        {
            Some(read) => read,
            None => {
                let stores = agents.stores.clone();
                let session = request.session.clone();
                spawn_blocking(move || stores.transcript(session.as_ref(), RETAINED_ENTRIES, None))
                    .await??
                    .context("session transcript is no longer on the machine")?
            }
        };

        for picture in read.pictures {
            agents.images.keep(request.session.as_ref(), picture);
        }
        (read.entries, read.earlier)
    };
    let next = earlier
        .checked_add(u64::try_from(entries.len())?)
        .context("transcript position overflow")?;
    Ok(SessionSnapshot {
        source,
        entries,
        earlier,
        cursor: SessionCursor {
            start: earlier,
            next,
            fingerprint: String::new(),
        },
        resumed: false,
    })
}

async fn paths(agents: &Agents, request: &FollowSession) -> Result<Vec<PathBuf>> {
    if agents.get(request.session.as_ref()).is_some() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();

    if let Some(file) = agents.session_file(request.session.as_ref()).await? {
        if request.session.as_ref().starts_with("subagent:claude:") {
            paths.push(file.with_extension("meta.json"));
        }
        if matches!(request.agent.as_ref(), "flsh" | "opencode") {
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
#[path = "../tests/agents_follow.rs"]
mod tests;
