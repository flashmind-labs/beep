//! The machine a session moves to: waits until the source ended it, brings
//! the conversation and the project's files, writes the transcript into the
//! harness's store and resumes the session in its own checkout.

use super::{
    Env, Harness,
    dial::dial,
    flsh,
    folder::{Folders, project_folder},
    progress::{Tracker, show},
    read::{self, Refused},
    resume::{Arrival, resume},
    saved::{Phase, Saved},
};
use crate::{
    agents::found::Agent,
    files::{
        sync::Syncs,
        syncthing::{Managed, Syncthing},
    },
};
use anyhow::{Context, Result, bail};
use beep_proto::node::{agent::MovePhase, link, mesh_state::SessionMove};
use std::{
    path::{Component, Path},
    time::Duration,
};
use tokio::time::{Instant, sleep};

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
}

/// What brings the project's files to this machine.
enum Arriving<'a> {
    Beep(&'a Syncs),
    /// Trusted: nothing here compares the folder with the source's.
    Syncthing(Managed),
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

    let folders = folders(env, record)?;

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
    // Decided before the source ends anything.
    let arriving = arriving(env, record, &folders.new).await?;
    log::info!(
        "session {}: it can move here, reading it from its source",
        record.id
    );
    let copy = env.state.copy(&record.id);
    let copied = conversation(env, record, tracker, &copy).await?;
    log::info!(
        "session {}: the conversation is here, waiting for the files",
        record.id
    );
    files(record, &arriving, tracker, copied.since).await?;
    log::info!("session {}: the files are here, resuming", record.id);
    show(tracker, |progress| progress.phase = MovePhase::Resuming);
    let arrival = Arrival {
        id: &record.id,
        mode: record.mode.clone(),
        model: record.model.clone(),
        worktrees: record.worktrees.clone(),
        from: record.from,
    };
    resume(env, &arrival, &kind, &folders, copied.file_name, copy).await
}

/// The session's folder in the source's checkout and in this machine's. A
/// session works in its project folder, never in a worktree: a record from
/// an app that still sent a worktree move resumes in the project folder.
fn folders(env: &Env, record: &SessionMove) -> Result<Folders> {
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

    Ok(Folders {
        old: Path::new(&theirs).join(relative),
        new: Path::new(&mine).join(relative),
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
                        since: Instant::now(),
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

/// How the project's files will come here: by Beep's sync with the source,
/// else by the Syncthing that keeps this machine's folder. Neither refuses.
async fn arriving<'a>(
    env: &'a Env,
    record: &SessionMove,
    folder: &Path,
) -> Result<Arriving<'a>> {
    if let Some(syncs) = &env.syncs
        && syncs.syncs_with(&record.project, &record.from).await
    {
        return Ok(Arriving::Beep(syncs));
    }

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
        .await?;
    syncs
        .handoff_for_move(&record.project, &record.from)
        .await
}
