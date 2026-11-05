//! The machine a session moves to: waits until the source ended it, brings
//! the conversation and the project's files, writes the transcript into the
//! harness's store and resumes the session in its own checkout.

use super::{
    Env, Harness,
    progress::{Tracker, show},
    read::{self, ALPN},
    rewrite,
    saved::{Phase, Saved},
    store,
};
use crate::agents::{Begin, found::Agent};
use anyhow::{Context, Result, bail};
use beep_mesh::Key;
use beep_proto::node::{
    agent::MovePhase,
    link,
    mesh_state::{SessionMove, StateValue, SyncSpec},
};
use iroh::EndpointAddr;
use std::{
    borrow::Cow,
    path::{Component, Path, PathBuf},
    time::Duration,
};
use tokio::{
    task::spawn_blocking,
    time::{Instant, sleep, timeout},
};

/// How long to wait for a connection to the source.
const CONNECT_WITHIN: Duration = Duration::from_secs(15);
/// The waits between tries to reach the source, doubling up to the last.
const FIRST_WAIT: Duration = Duration::from_secs(1);
const LAST_WAIT: Duration = Duration::from_secs(10);

/// The conversation is all here: when the source was heard to have ended
/// the session, and the name it keeps the transcript under.
struct Copied {
    since: Instant,
    file_name: String,
}

/// The folder the session worked in on the source and the folder it works in
/// here.
struct Folders {
    old: PathBuf,
    new: PathBuf,
}

pub(super) async fn run(env: &Env, record: &SessionMove, tracker: &Tracker) -> Result<()> {
    let saved = env.state.load(&record.id);
    if saved
        .as_ref()
        .is_some_and(|saved| saved.at == record.at && saved.phase != Phase::Working)
    {
        return Ok(());
    }
    let result = move_here(env, record, tracker).await;
    let (phase, reason) = match &result {
        Ok(()) => (Phase::Done, None),
        Err(e) => (Phase::Failed, Some(format!("{e:#}"))),
    };
    show(tracker, |progress| match &reason {
        None => progress.phase = MovePhase::Done,
        Some(reason) => {
            progress.phase = MovePhase::Failed;
            progress.reason = Some(reason.clone());
        }
    });
    env.state.clear(&record.id, false);
    env.state
        .save(
            &record.id,
            &Saved {
                at: record.at,
                phase,
                reason,
            },
        )
        .context("keeping how the move ended")?;
    result
}

async fn move_here(env: &Env, record: &SessionMove, tracker: &Tracker) -> Result<()> {
    let agent = Agent::from_name(record.agent.as_str());
    let harness = agent
        .and_then(Harness::of)
        .with_context(|| format!("{} sessions cannot move yet", record.agent))?;
    let folders = folders(env, record)?;
    if !folders.new.is_dir() {
        bail!(
            "{} has no checkout on this machine to resume in",
            record.project
        );
    }
    let kind = env
        .agents
        .kinds
        .iter()
        .find(|kind| kind.name == record.agent.as_str())
        .with_context(|| format!("this machine has no {}", record.agent))?
        .clone();
    if env.agents.runs(&record.id) {
        // A restart between starting and noting it.
        return Ok(());
    }
    let copy = env.state.copy(&record.id);
    let copied = conversation(env, record, tracker, &copy).await?;
    log::info!(
        "session {}: the conversation is here, waiting for the files",
        record.id
    );
    files(env, record, tracker, copied.since).await?;
    log::info!("session {}: the files are here, resuming", record.id);
    show(tracker, |progress| progress.phase = MovePhase::Resuming);
    let stored = {
        let (id, old, new) = (record.id.clone(), folders.old.clone(), folders.new.clone());
        spawn_blocking(move || -> Result<_> {
            let bytes = std::fs::read(&copy)?;
            Ok(rewrite(harness, &bytes, &id, &copied.file_name, &old, &new))
        })
        .await??
    };
    let path = env.roots.path(harness, &stored);
    let at = path.clone();
    spawn_blocking(move || store::write(&at, &stored)).await??;
    if let Some(agent) = agent {
        env.agents
            .spend
            .mark_counted(agent, &record.id, &path)
            .context("marking the copied usage as counted")?;
    }
    let cwd = folders.new.to_string_lossy().into_owned();
    env.agents.start(
        &kind,
        Some(&cwd),
        Begin {
            resume: Some(record.id.clone()),
            mode: record.mode.clone(),
            note: Some(format!("Moved here from {}", env.name_of(&record.from))),
            brief: Some(Cow::Owned(format!(
                "This session moved from another machine. Paths in the conversation so far name its old folder, {}; the same checkout is now {}.",
                folders.old.display(),
                folders.new.display()
            ))),
            ..Begin::default()
        },
    )?;
    Ok(())
}

/// The old and new folders, from the project's `sync/<name>` record.
fn folders(env: &Env, record: &SessionMove) -> Result<Folders> {
    let bytes = env
        .mesh
        .get(&Key::Sync(record.project.clone()))
        .with_context(|| format!("{} is not a synced project", record.project))?;
    let spec = SyncSpec::from_bytes(&bytes).context("the project's record does not read")?;
    let (me, from) = (env.mesh.me(), record.from);
    let theirs = spec
        .folders
        .get(&from)
        .context("the source does not keep the project")?;
    let mine = spec
        .folders
        .get(&me)
        .context("this machine does not keep the project")?;
    let relative = Path::new(&record.path);
    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        bail!("the session's folder is not inside the project");
    }
    Ok(Folders {
        old: Path::new(theirs).join(relative),
        new: Path::new(mine).join(relative),
    })
}

/// Copies the conversation until the source says the session ended, and
/// answers when that was.
async fn conversation(
    env: &Env,
    record: &SessionMove,
    tracker: &Tracker,
    copy: &Path,
) -> Result<Copied> {
    if let Some(dir) = copy.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let mut wait = FIRST_WAIT;
    loop {
        show(tracker, |progress| {
            if progress.phase != MovePhase::Waiting {
                progress.phase = MovePhase::Waiting;
            }
        });
        match pull_once(env, record, copy).await {
            Ok(pulled) => {
                wait = FIRST_WAIT;
                show(tracker, |progress| {
                    progress.source_away = false;
                    progress.copied = pulled.size;
                    progress.total = pulled.size;
                    if !pulled.live {
                        progress.phase = MovePhase::Conversation;
                    }
                });
                if !pulled.live {
                    return Ok(Copied {
                        since: Instant::now(),
                        file_name: pulled.file_name,
                    });
                }
            }
            Err(e) => {
                log::info!("session {} not read from its source yet: {e:#}", record.id);
                show(tracker, |progress| progress.source_away = true);
                wait = (wait * 2).min(LAST_WAIT);
            }
        }
        sleep(wait).await;
    }
}

async fn pull_once(env: &Env, record: &SessionMove, copy: &Path) -> Result<read::Pulled> {
    let addr = env
        .mesh
        .devices()
        .into_iter()
        .find(|status| status.device.id == record.from)
        .and_then(|status| status.addr)
        .unwrap_or_else(|| EndpointAddr::new(record.from));
    let connection = timeout(CONNECT_WITHIN, env.endpoint.connect(addr, ALPN))
        .await
        .context("could not reach the source in time")??;
    let (send, recv) = connection.open_bi().await?;
    let (tx, rx) = link(send, recv);
    let pulled = read::pull(tx, rx, record.agent.clone(), &record.id, copy).await;
    connection.close(0u32.into(), b"done");
    pulled
}

/// Waits for the project's files to be here.
async fn files(env: &Env, record: &SessionMove, tracker: &Tracker, since: Instant) -> Result<()> {
    show(tracker, |progress| progress.phase = MovePhase::Files);
    let syncs = env
        .syncs
        .as_ref()
        .context("this machine shares no folders")?;
    syncs
        .have_all_from(&record.project, &record.from, since, |status| {
            let left = status.pending.max(status.waiting.len() as u64);
            show(tracker, |progress| progress.files_left = left);
        })
        .await
}
