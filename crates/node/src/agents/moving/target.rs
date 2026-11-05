//! The machine a session moves to: waits until the source ended it, brings
//! the conversation and the project's files, writes the transcript into the
//! harness's store and resumes the session in its own checkout.

use super::{
    Env, Harness,
    dial::dial,
    progress::{Tracker, show},
    read::{self, Refused},
    resume::{Arrival, Folders as ResumeFolders, How, resume},
    saved::{MadeWorktree, Phase, Saved},
    worktree::{self, Work},
};
use crate::agents::found::Agent;
use anyhow::{Context, Result, bail};
use beep_mesh::Key;
use beep_proto::node::{
    agent::MovePhase,
    link,
    mesh_state::{SessionMove, StateValue, SyncSpec},
};
use std::{
    path::{Component, Path, PathBuf},
    time::Duration,
};
use tokio::{
    task::spawn_blocking,
    time::{Instant, sleep},
};

/// The waits between tries to reach the source, doubling up to the last.
const FIRST_WAIT: Duration = Duration::from_secs(1);
const LAST_WAIT: Duration = Duration::from_secs(10);

/// The conversation is all here: when the source was heard to have ended
/// the session, and the name it keeps the transcript under.
struct Copied {
    since: Instant,
    file_name: String,
    /// The worktree's commit and uncommitted work, for a worktree session.
    work: Option<Work>,
}

/// The folder the session worked in on the source and the folder it works in
/// here.
struct Folders {
    old: PathBuf,
    new: PathBuf,
    /// For a worktree session: this machine's checkout, and the worktree
    /// the session works in here, which is made when it moves.
    worktree: Option<Made>,
}

/// The worktree a target makes.
struct Made {
    checkout: PathBuf,
    folder: PathBuf,
}

pub(super) async fn run(env: &Env, record: &SessionMove, tracker: &Tracker) -> Result<()> {
    let saved = env.state.load(&record.id);
    if let Some(saved) = saved.filter(|saved| saved.is_over(record.at)) {
        // After a restart, how it ended is still what the source and the
        // desktop are told.
        show(tracker, |progress| match saved.phase {
            Phase::Done => progress.phase = MovePhase::Done,
            _ => {
                progress.phase = MovePhase::Failed;
                progress.reason = saved.reason;
            }
        });
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
        .save(&record.id, &Saved::over(record.at, phase, reason))
        .context("keeping how the move ended")?;
    result
}

async fn move_here(env: &Env, record: &SessionMove, tracker: &Tracker) -> Result<()> {
    if Agent::from_name(record.agent.as_str())
        .and_then(Harness::of)
        .is_none()
    {
        bail!("{} sessions cannot move yet", record.agent);
    }
    let folders = folders(env, record)?;
    match &folders.worktree {
        Some(made) => {
            if !made.checkout.is_dir() {
                bail!(
                    "{} has no checkout on this machine to make a worktree in",
                    record.project
                );
            }
            if let Some(worktree) = &record.worktree
                && env
                    .agents
                    .moves
                    .ignores_worktree(&record.project, &worktree.branch)
                    .await
            {
                bail!("Syncthing is set not to sync this worktree, so it cannot move here");
            }
            // What an earlier try made for this move goes first.
            take_back(env, &record.id, record.at).await;
            // Whether the branch and folder allow it is known before the
            // source ends anything.
            if let Some(worktree) = &record.worktree {
                let (checkout, folder, branch) = (
                    made.checkout.clone(),
                    made.folder.clone(),
                    worktree.branch.clone(),
                );
                spawn_blocking(move || worktree::check(&checkout, &folder, &branch)).await??;
            }
        }
        None if !folders.new.is_dir() => bail!(
            "{} has no checkout on this machine to resume in",
            record.project
        ),
        None => {}
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
    if env.syncs.is_none() {
        bail!("this machine shares no folders, so the project's files cannot come here");
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
    let made = match (&folders.worktree, &record.worktree, &copied.work) {
        (Some(made), Some(worktree), Some(work)) => {
            let (checkout, folder, branch, work) = (
                made.checkout.clone(),
                made.folder.clone(),
                worktree.branch.clone(),
                work.clone(),
            );
            let placed =
                spawn_blocking(move || worktree::attach(&checkout, &folder, &branch, &work))
                    .await?
                    .context("making the worktree")?;
            // Kept, so a restart or a cancel before the session runs
            // takes the worktree back.
            env.state
                .save(
                    &record.id,
                    &Saved {
                        at: record.at,
                        phase: Phase::Working,
                        reason: None,
                        open: None,
                        made: Some(MadeWorktree {
                            checkout: made.checkout.clone(),
                            folder: made.folder.clone(),
                            placed,
                        }),
                    },
                )
                .context("keeping the worktree made for the move")?;
            Some((made, placed))
        }
        (Some(_), ..) => bail!("the source sent no worktree state"),
        _ => None,
    };
    let arrival = Arrival {
        id: &record.id,
        mode: record.mode.clone(),
        from: record.from,
        how: How::Moved,
    };
    let placed = ResumeFolders {
        old: folders.old.clone(),
        new: folders.new.clone(),
    };
    let resumed = resume(env, &arrival, &kind, &placed, copied.file_name, copy).await;
    if resumed.is_err()
        && let Some((made, placed)) = made
    {
        let (checkout, folder) = (made.checkout.clone(), made.folder.clone());
        if let Err(e) = spawn_blocking(move || worktree::undo(&checkout, &folder, placed)).await {
            log::warn!("the worktree made for a move stays: {e}");
        }
    }
    resumed
}

/// Takes back the worktree an earlier try of the move made, when the
/// session does not run: a restart or a cancel came between making it and
/// starting the session, and it still holds the patch.
pub(super) async fn take_back(env: &Env, id: &str, at: u64) {
    let Some(made) = env
        .state
        .load(id)
        .filter(|saved| saved.at == at)
        .and_then(|saved| saved.made)
    else {
        return;
    };
    if env.agents.runs(id) {
        return;
    }
    let done = spawn_blocking(move || worktree::undo(&made.checkout, &made.folder, made.placed));
    if let Err(e) = done.await {
        log::warn!("the worktree of session {id} was not taken back: {e}");
    }
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
    let Some(worktree) = &record.worktree else {
        return Ok(Folders {
            old: Path::new(theirs).join(relative),
            new: Path::new(mine).join(relative),
            worktree: None,
        });
    };
    let top = Path::new(mine).join(beep_proto::node::agent::worktree_path(&worktree.branch));
    Ok(Folders {
        old: Path::new(&worktree.from).join(relative),
        new: top.join(relative),
        worktree: Some(Made {
            checkout: PathBuf::from(mine),
            folder: top,
        }),
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
                // The source let a read through: the session can move.
                env.agents.moves.take(&record.id);
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
                        work: pulled.work,
                    });
                }
            }
            // The source will not serve it, as it is: waiting changes nothing.
            Err(e) if e.is::<Refused>() => return Err(e),
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
    let connection = dial(env, record.from).await?;
    let (send, recv) = connection.open_bi().await?;
    let (tx, rx) = link(send, recv);
    let pulled = read::pull(
        tx,
        rx,
        read::Ask {
            agent: record.agent.clone(),
            id: &record.id,
            copy,
            kind: read::Kind::Move {
                worktree: record.worktree.is_some(),
            },
        },
    )
    .await;
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
            // Why the files do not come, such as two checkouts at different
            // commits, rather than a count that does not move.
            let reason = status.error.clone();
            show(tracker, |progress| {
                progress.files_left = left;
                progress.reason = reason;
            });
        })
        .await
}
