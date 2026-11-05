//! The machine a session moves to: waits until the source ended it, brings
//! the conversation and the project's files, writes the transcript into the
//! harness's store and resumes the session in its own checkout.

use super::{
    Env, Harness,
    dial::dial,
    offers::project_folder,
    progress::{Tracker, show},
    read::{self, Refused},
    resume::{Arrival, Folders as ResumeFolders, How, resume},
    saved::{MadeWorktree, Phase, Saved},
    worktree::{self, Undone, Work},
};
use crate::{
    agents::found::Agent,
    files::{
        sync::Syncs,
        syncthing::{Managed, Syncthing},
    },
};
use anyhow::{Context, Result, bail};
use beep_proto::node::{
    agent::{MovePhase, Status},
    link,
    mesh_state::SessionMove,
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
/// How often this machine's Syncthing is asked whether a folder is complete.
const SYNCTHING_LOOK: Duration = Duration::from_secs(1);
/// How long a move waits for it before it goes on anyway.
const SYNCTHING_WAIT: Duration = Duration::from_secs(120);

/// The conversation is all here: when the source was heard to have ended
/// the session, and the name it keeps the transcript under.
struct Copied {
    since: Instant,
    file_name: String,
    /// The worktree's commit and uncommitted work, for a worktree session.
    work: Option<Work>,
}

/// What brings the project's files to this machine.
enum Arriving<'a> {
    Beep(&'a Syncs),
    /// Trusted: nothing here compares the folder with the source's.
    Syncthing(Managed),
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
    // What an earlier try, or an earlier move, made here and left goes first.
    take_back(env, &record.id).await;
    env.agents.check_room()?;
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
            // Whether the branch and folder allow it is known before the
            // source ends anything.
            if let Some(worktree) = &record.worktree {
                let running = runs_in(env, &made.folder);
                let (checkout, folder, branch) = (
                    made.checkout.clone(),
                    made.folder.clone(),
                    worktree.branch.clone(),
                );
                spawn_blocking(move || worktree::check(&checkout, &folder, &branch, running))
                    .await??;
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
    // Decided before the source ends anything.
    let arriving = arriving(env, record, &folders).await?;
    let copy = env.state.copy(&record.id);
    let copied = conversation(env, record, tracker, &copy).await?;
    log::info!(
        "session {}: the conversation is here, waiting for the files",
        record.id
    );
    files(record, &arriving, tracker, copied.since).await?;
    log::info!("session {}: the files are here, resuming", record.id);
    show(tracker, |progress| progress.phase = MovePhase::Resuming);
    let made = match (&folders.worktree, &record.worktree, &copied.work) {
        (Some(made), Some(worktree), Some(work)) => {
            let running = runs_in(env, &made.folder);
            let (checkout, folder, branch, work) = (
                made.checkout.clone(),
                made.folder.clone(),
                worktree.branch.clone(),
                work.clone(),
            );
            // Noted before anything is made, so a restart or a crash inside
            // `attach` finds what to take back; with nothing applied yet, a
            // folder that is clean is all it may remove.
            let (c, f, b) = (checkout.clone(), folder.clone(), branch.clone());
            let plan = spawn_blocking(move || worktree::check(&c, &f, &b, running)).await??;
            env.state
                .save(
                    &record.id,
                    &Saved {
                        at: record.at,
                        phase: Phase::Working,
                        reason: None,
                        open: None,
                        made: Some(MadeWorktree {
                            checkout: checkout.clone(),
                            folder: folder.clone(),
                            placed: plan,
                            applied: String::new(),
                        }),
                    },
                )
                .context("keeping what the move is about to make")?;
            let attached = spawn_blocking(move || {
                worktree::attach(&checkout, &folder, &branch, &work, running)
            })
            .await?
            .context("making the worktree");
            let attached = match attached {
                Ok(attached) => attached,
                Err(e) => {
                    // `attach` leaves nothing changed when it fails.
                    let mut saved = env
                        .state
                        .load(&record.id)
                        .unwrap_or_else(|| Saved::over(record.at, Phase::Working, None));
                    saved.made = None;
                    if let Err(e) = env.state.save(&record.id, &saved) {
                        log::warn!("the pending note of session {} stays: {e}", record.id);
                    }
                    return Err(e);
                }
            };
            let note = MadeWorktree {
                checkout: made.checkout.clone(),
                folder: made.folder.clone(),
                placed: attached.placed,
                applied: attached.applied,
            };
            // Kept, so a restart or a cancel before the session runs
            // takes the worktree back.
            let kept = env.state.save(
                &record.id,
                &Saved {
                    at: record.at,
                    phase: Phase::Working,
                    reason: None,
                    open: None,
                    made: Some(note.clone()),
                },
            );
            if let Err(e) = kept {
                // Nothing would remember it: back out now, while it holds
                // only what was put there.
                undo_logged(&record.id, note).await;
                return Err(e).context("keeping the worktree made for the move");
            }
            true
        }
        (Some(_), ..) => bail!("the source sent no worktree state"),
        _ => false,
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
    if resumed.is_err() && made {
        take_back(env, &record.id).await;
    }
    resumed
}

/// Whether a session of this node runs in `folder` or below it.
fn runs_in(env: &Env, folder: &Path) -> bool {
    env.agents
        .list()
        .iter()
        .any(|info| info.state.status != Status::Ended && Path::new(&info.cwd).starts_with(folder))
}

/// Takes back the worktree an earlier try of a move made, once: a restart,
/// a cancel or a failure came between making it and running the session. It
/// goes only when no session runs in it and it holds exactly what was put
/// there; anything else is the user's and stays, with a line in the log.
pub(super) async fn take_back(env: &Env, id: &str) {
    // Sessions that run again after a restart are not running yet.
    env.agents.until_reopened().await;
    let Some(mut saved) = env.state.load(id) else {
        return;
    };
    let Some(made) = saved.made.take() else {
        return;
    };
    // Forgotten first, so this runs once whatever happens next.
    if let Err(e) = env.state.save(id, &saved) {
        log::warn!("the worktree of session {id} was not taken back, it could not be noted: {e}");
        return;
    }
    if env.agents.runs(id) || runs_in(env, &made.folder) {
        log::info!(
            "the worktree of session {id} stays: a session runs in {}",
            made.folder.display()
        );
        return;
    }
    undo_logged(id, made).await;
}

async fn undo_logged(id: &str, made: MadeWorktree) {
    let folder = made.folder.clone();
    let done = spawn_blocking(move || {
        worktree::undo(&made.checkout, &made.folder, &made.placed, &made.applied)
    })
    .await;
    match done {
        Ok(Undone::Removed | Undone::Reset) => {
            log::info!(
                "the worktree {} made for session {id} was taken back",
                folder.display()
            );
        }
        Ok(Undone::Left(why)) => log::warn!("the worktree of session {id} stays: {why}"),
        Err(e) => log::warn!("the worktree of session {id} was not taken back: {e}"),
    }
}

/// The old and new folders.
fn folders(env: &Env, record: &SessionMove) -> Result<Folders> {
    let theirs = project_folder(&env.mesh, &record.project, &record.from)
        .context("the source does not keep the project")?;
    let mine = project_folder(&env.mesh, &record.project, &env.mesh.me())
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
            old: Path::new(&theirs).join(relative),
            new: Path::new(&mine).join(relative),
            worktree: None,
        });
    };
    let top = Path::new(&mine).join(beep_proto::node::agent::worktree_path(&worktree.branch));
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

/// How the project's files will come here: by Beep's sync with the source,
/// else by the Syncthing that keeps this machine's folder. Neither refuses.
async fn arriving<'a>(
    env: &'a Env,
    record: &SessionMove,
    folders: &Folders,
) -> Result<Arriving<'a>> {
    if let Some(syncs) = &env.syncs
        && syncs.syncs_with(&record.project, &record.from).await
    {
        return Ok(Arriving::Beep(syncs));
    }
    let folder = folders
        .worktree
        .as_ref()
        .map_or(&folders.new, |made| &made.checkout);
    match Syncthing::local().check(folder).await {
        Some(managed) => Ok(Arriving::Syncthing(managed)),
        None => bail!(
            "{} is not synced between machines: turn on sync on its page",
            record.project
        ),
    }
}

/// Waits until this machine's Syncthing says the folder is complete, for
/// `SYNCTHING_WAIT` at most. A folder known only by its marker has no ID to
/// ask about, and goes on at once.
async fn syncthing_done(record: &SessionMove, managed: &Managed, tracker: &Tracker) {
    let Some(id) = &managed.id else {
        return;
    };
    let syncthing = Syncthing::local();
    let until = Instant::now() + SYNCTHING_WAIT;
    loop {
        match syncthing.folder_status(id).await {
            Ok(status) => {
                show(tracker, |progress| {
                    progress.files_left = status.need_total_items;
                });
                if status.is_complete() {
                    return;
                }
            }
            Err(e) => log::debug!("Syncthing did not say how {id} stands: {e:#}"),
        }
        if Instant::now() >= until {
            log::warn!(
                "session {}: Syncthing has not finished {} in {}s, resuming anyway",
                record.id,
                managed.label,
                SYNCTHING_WAIT.as_secs()
            );
            return;
        }
        sleep(SYNCTHING_LOOK).await;
    }
}

/// Waits for the project's files to be here.
async fn files(
    record: &SessionMove,
    arriving: &Arriving<'_>,
    tracker: &Tracker,
    since: Instant,
) -> Result<()> {
    show(tracker, |progress| progress.phase = MovePhase::Files);
    let syncs = match arriving {
        Arriving::Beep(syncs) => syncs,
        Arriving::Syncthing(managed) => {
            syncthing_done(record, managed, tracker).await;
            return Ok(());
        }
    };
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
