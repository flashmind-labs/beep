//! The machine a session moves to: waits until the source ended it, brings
//! the conversation and the project's files, writes the transcript into the
//! harness's store and resumes the session in its own checkout.

use super::{
    Env, Harness,
    dial::dial,
    flsh,
    folder::{Folders, project_folder},
    progress::{Tracker, show},
    read::{self, Checkout, MovingWorktree, Refused},
    resume::{Arrival, resume},
    saved::{Phase, Saved},
    worktree,
};
use crate::agents::found::Agent;
use anyhow::{Context, Result, bail};
use beep_proto::node::{agent::MovePhase, link, mesh_state::SessionMove};
use std::{
    path::{Component, Path, PathBuf},
    time::Duration,
};
use tokio::{task::spawn_blocking, time::sleep};

/// The waits between tries to reach the source, doubling up to the last.
const FIRST_WAIT: Duration = Duration::from_secs(1);
const LAST_WAIT: Duration = Duration::from_secs(10);
/// How often the checkouts are compared while they differ.
const CHECKOUT_LOOK: Duration = Duration::from_secs(2);

/// The conversation is all here, and the name the source keeps the
/// transcript under.
struct Copied {
    file_name: String,
}

pub(super) async fn run(env: &Env, record: &SessionMove, tracker: &Tracker) -> Result<()> {
    let saved = env.state.load(&record.id);

    if let Some(saved) = saved.filter(|saved| saved.is_over(record.at)) {
        // After a restart, how it ended is still what the source and the
        // desktop are told.
        show_end(tracker, &saved.phase, saved.reason);
        return Ok(());
    }

    let result = move_here(env, record, tracker).await;
    let (phase, reason) = match &result {
        Ok(()) => (Phase::Done, None),
        Err(e) => (Phase::Failed, Some(format!("{e:#}"))),
    };
    show_end(tracker, &phase, reason.clone());
    env.state.clear(&record.id, false);
    env.state
        .save(&record.id, &Saved::over(record.at, phase, reason))
        .context("keeping how the move ended")?;
    result
}

fn show_end(tracker: &Tracker, phase: &Phase, reason: Option<String>) {
    show(tracker, |progress| match phase {
        Phase::Done => progress.phase = MovePhase::Done,
        _ => {
            progress.phase = MovePhase::Failed;
            progress.reason = reason;
        }
    });
}

async fn move_here(env: &Env, record: &SessionMove, tracker: &Tracker) -> Result<()> {
    if record
        .agent
        .as_ref()
        .parse::<Agent>()
        .ok()
        .and_then(|agent| Harness::try_from(agent).ok())
        .is_none()
    {
        bail!("{} sessions cannot move yet", record.agent);
    }

    let (folders, roots) = folders(env, record)?;

    if !folders.new.is_dir() {
        bail!(
            "{} has no checkout on this machine to resume in",
            record.project
        );
    }

    let kind = env
        .agents
        .named(record.agent.as_ref())
        .with_context(|| format!("this machine has no {}", record.agent))?;
    if kind.name.parse::<Agent>().ok() == Some(Agent::Flsh) {
        flsh::check(&kind, &env.name_of(&env.mesh.me())).await?;
    }

    if env.agents.runs(&record.id) {
        // A restart between starting and noting it.
        return Ok(());
    }
    log::info!(
        "session {}: it can move here, reading it from its source",
        record.id
    );
    let copy = env.state.copy(&record.id);
    let copied = conversation(env, record, tracker, &copy).await?;
    log::info!(
        "session {}: the conversation is here, waiting for the checkouts",
        record.id
    );
    let checkout = checkouts(env, record, tracker, &roots.new).await?;
    let moved: Vec<String> = checkout
        .worktrees
        .iter()
        .map(|tree| roots.new.join(&tree.path).to_string_lossy().into_owned())
        .collect();
    worktrees(env, record, tracker, &roots.new, checkout.worktrees).await?;
    log::info!("session {}: the worktrees are here, resuming", record.id);
    show(tracker, |progress| progress.phase = MovePhase::Resuming);
    let arrival = Arrival {
        id: &record.id,
        mode: record.mode.clone(),
        model: record.model.clone(),
        worktrees: moved,
        from: record.from,
    };
    resume(env, &arrival, &kind, &folders, copied.file_name, copy).await
}

/// The session's folder in the source's checkout and in this machine's, and
/// the two checkouts themselves. A session works in its project folder,
/// never in a worktree: a record from an app that still sent a worktree
/// move resumes in the project folder.
fn folders(env: &Env, record: &SessionMove) -> Result<(Folders, Folders)> {
    let theirs = project_folder(&env.mesh, &record.project, &record.from)
        .context("the source does not keep the project")?;
    let mine = project_folder(&env.mesh, &record.project, &env.mesh.me())
        .context("this machine does not keep the project")?;
    let relative = match record.worktree {
        Some(_) => Path::new(""),
        None => Path::new(&record.path),
    };

    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        bail!("the session's folder is not inside the project");
    }

    let roots = Folders {
        old: PathBuf::from(&theirs),
        new: PathBuf::from(&mine),
    };
    let folders = Folders {
        old: roots.old.join(relative),
        new: roots.new.join(relative),
    };
    Ok((folders, roots))
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
    // Whether the last read reached the source, so each change is logged
    // once rather than on every try.
    let mut reached = None;

    loop {
        show(tracker, |progress| {
            if progress.phase != MovePhase::Waiting {
                progress.phase = MovePhase::Waiting;
            }
        });

        match pull_once(env, record, copy).await {
            Ok(pulled) => {
                wait = FIRST_WAIT;

                if reached != Some(true) {
                    log::info!(
                        "session {}: read {} bytes from its source, which {}",
                        record.id,
                        pulled.size,
                        if pulled.live {
                            "still runs it; the move is taken"
                        } else {
                            "ended it"
                        }
                    );
                    reached = Some(true);
                }
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
                        file_name: pulled.file_name,
                    });
                }
            }
            // The source will not serve it, as it is: waiting changes nothing.
            Err(e) if e.is::<Refused>() => return Err(e),
            Err(e) => {
                if reached != Some(false) {
                    log::info!("session {} not read from its source yet: {e:#}", record.id);
                    reached = Some(false);
                } else {
                    log::debug!("session {} not read from its source yet: {e:#}", record.id);
                }

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
        },
    )
    .await;
    connection.close(0u32.into(), b"done");
    pulled
}

/// Waits until this machine's checkout of the project is at the source's
/// commit, saying both while they differ, and answers what the source said.
async fn checkouts(
    env: &Env,
    record: &SessionMove,
    tracker: &Tracker,
    root: &Path,
) -> Result<Checkout> {
    show(tracker, |progress| progress.phase = MovePhase::Files);
    let from = env.name_of(&record.from);
    let mut wait = FIRST_WAIT;

    loop {
        let theirs = match ask_checkout(env, record).await {
            Ok(theirs) => theirs,
            Err(e) if e.is::<Refused>() => return Err(e),
            Err(e) => {
                log::debug!("session {}: the source's checkout not read: {e:#}", record.id);
                show(tracker, |progress| progress.source_away = true);
                sleep(wait).await;
                wait = (wait * 2).min(LAST_WAIT);
                continue;
            }
        };
        let dir = root.to_owned();
        let ours = spawn_blocking(move || worktree::head(&dir)).await??;

        if theirs.head == ours {
            show(tracker, |progress| {
                progress.source_away = false;
                progress.reason = None;
            });
            return Ok(theirs);
        }

        let reason = format!(
            "Waiting for checkouts to match: {from} at {}, here {}",
            short(theirs.head.as_deref()),
            short(ours.as_deref())
        );
        show(tracker, |progress| {
            progress.source_away = false;
            progress.reason = Some(reason);
        });
        wait = FIRST_WAIT;
        sleep(CHECKOUT_LOOK).await;
    }
}

/// A commit as people read it, or that there is none.
fn short(head: Option<&str>) -> String {
    head.map_or_else(
        || "no commit".to_owned(),
        |head| head.chars().take(7).collect(),
    )
}

async fn ask_checkout(env: &Env, record: &SessionMove) -> Result<Checkout> {
    let connection = dial(env, record.from).await?;
    let (send, recv) = connection.open_bi().await?;
    let (tx, rx) = link(send, recv);
    let checkout = read::checkout(tx, rx, &record.id).await;
    connection.close(0u32.into(), b"done");
    checkout
}

/// Brings each of the session's worktrees from the source and makes it
/// here, at the same folder in the project's checkout `root`.
async fn worktrees(
    env: &Env,
    record: &SessionMove,
    tracker: &Tracker,
    root: &Path,
    trees: Vec<MovingWorktree>,
) -> Result<()> {
    let total = u32::try_from(trees.len()).unwrap_or(u32::MAX);

    for (at, tree) in (1..).zip(trees) {
        let relative = Path::new(&tree.path);

        if !relative.components().all(|part| matches!(part, Component::Normal(_))) {
            bail!("the worktree {} is not inside the project", tree.path);
        }

        let name = relative
            .file_name()
            .map_or_else(|| tree.path.clone(), |name| name.to_string_lossy().into_owned());
        show(tracker, |progress| {
            progress.worktree = Some(name.clone());
            progress.worktree_at = at;
            progress.worktrees = total;
        });
        let (main, head) = (root.to_owned(), tree.head.clone());
        let bundle = !spawn_blocking(move || worktree::has_commit(&main, &head)).await?;
        let fetched = fetch_worktree(env, record, &tree.path, bundle)
            .await
            .with_context(|| format!("bringing the worktree {name}"))?;
        let (main, path) = (root.to_owned(), root.join(relative));
        spawn_blocking(move || -> Result<()> {
            if let Some(bytes) = &fetched.bundle {
                worktree::import(&main, &fetched.work.head, bytes)?;
            }

            worktree::attach(&main, &path, &tree.branch, &fetched.work).map(|_| ())
        })
        .await?
        .with_context(|| format!("making the worktree {name} here"))?;
        log::info!("session {}: the worktree {name} is here", record.id);
    }

    show(tracker, |progress| progress.worktree = None);
    Ok(())
}

async fn fetch_worktree(
    env: &Env,
    record: &SessionMove,
    path: &str,
    bundle: bool,
) -> Result<read::Fetched> {
    let connection = dial(env, record.from).await?;
    let (send, recv) = connection.open_bi().await?;
    let (tx, rx) = link(send, recv);
    let fetched = read::worktree(tx, rx, &record.id, path, bundle).await;
    connection.close(0u32.into(), b"done");
    fetched
}
