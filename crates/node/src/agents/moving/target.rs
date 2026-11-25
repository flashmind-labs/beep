//! The machine a session moves to: waits until the source ended it, brings
//! the conversation and the project's files, writes the transcript into the
//! harness's store and resumes the session in its own checkout. A session
//! of a chat has a folder of its own in place of a checkout: what it holds
//! comes over, and a session that had ended is kept ended.

use super::{
    Env, Harness,
    dial::with_stream,
    files, flsh,
    folder::{Folders, project_folder},
    progress::{Tracker, show},
    read::{self, Checkout, Refused},
    resume::{Arrival, kept_here, resume, store},
    saved::{Folder, Phase, PlacedTree, Saved},
    worktree,
};
use crate::agents::{SavedSession, checkouts::watch_paths, conversation, found::Agent};
use crate::events::paths::Paths;
use anyhow::{Context, Result, bail};
use beep_proto::SessionId;
use beep_proto::node::{agent::MovePhase, mesh_state::SessionMove};
use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    sync::Notify,
    task::spawn_blocking,
    time::sleep,
};

/// The waits between tries to reach the source, doubling up to the last.
const FIRST_WAIT: Duration = Duration::from_secs(1);
const LAST_WAIT: Duration = Duration::from_secs(10);
/// The longest a chat's files go untold while they come in.
const TELL_EVERY: Duration = Duration::from_millis(250);

pub(super) async fn run(env: &Env, record: &SessionMove, tracker: &Tracker) -> Result<()> {
    let saved = env.state.load(&record.id);

    if let Some(saved) = saved
        && saved.is_over(record.at)
    {
        // After a restart, how it ended is still what the source and the
        // desktop are told.
        show_end(tracker, &saved.phase, saved.reason);
        return Ok(());
    }

    let result = move_here(env, record, tracker).await;
    if !tracker.settle() {
        // The follower claimed cancellation and owns the take-back.
        return Ok(());
    }

    let (phase, reason) = match &result {
        Ok(()) => (Phase::Done, None),
        Err(e) => {
            take_back(env, &record.id, record.at).await;
            (Phase::Failed, Some(format!("{e:#}")))
        }
    };
    let saved = env
        .state
        .save(&record.id, &Saved::over(record.at, phase, reason.clone()));
    env.state.clear_copy(&record.id);
    show_end(tracker, &phase, reason);
    saved.context("keeping how the move ended")?;
    result
}

fn show_end(tracker: &Tracker, phase: &Phase, reason: Option<String>) {
    let phase = if *phase == Phase::Done {
        MovePhase::Done
    } else {
        MovePhase::Failed
    };
    tracker.finish(phase, reason);
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
    let chat = record.conversation.as_ref();

    if chat.is_none() && !folders.new.is_dir() {
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
    let file_name = conversation(env, record, tracker, &copy).await?;
    let (saved, moved, archive) = match chat {
        Some(_) => {
            log::info!(
                "session {}: the conversation is here, bringing its files",
                record.id
            );
            let (saved, archive) = chat_files(env, record, tracker).await?;
            (saved, Vec::new(), Some(archive))
        }
        None => {
            log::info!(
                "session {}: the conversation is here, waiting for the checkouts",
                record.id
            );
            let mut checkout = checkouts(env, record, tracker, &roots.new).await?;
            let saved = checkout.session.take();
            let moved: Vec<String> = checkout
                .worktrees
                .iter()
                .map(|tree| roots.new.join(&tree.path).to_string_lossy().into_owned())
                .collect();
            worktrees(env, record, tracker, &roots.new, checkout).await?;
            (saved, moved, None)
        }
    };
    anyhow::ensure!(tracker.resume(), "the move was cancelled");
    if let Some(archive) = archive {
        let folder = folders.new.clone();
        spawn_blocking(move || {
            let archive = std::fs::File::open(archive)?;
            files::unpack(archive, &folder)
        })
        .await?
        .context("unpacking the chat's files")?;
    }

    log::info!("session {}: its files are here, resuming", record.id);
    let arrival = Arrival {
        id: &record.id,
        mode: record.mode.clone(),
        model: record.model.clone(),
        worktrees: moved,
        from: record.from,
        saved,
        chat: chat.is_some(),
    };
    store(env, &record.id, &kind, &folders, file_name, copy).await?;

    if chat.is_some_and(|chat| chat.ended) {
        // It lists as ended from the store; what the source saved of it
        // keeps its name and its place under its chat.
        if let Some(saved) = kept_here(&arrival, &folders.new, env.mesh.me()) {
            env.agents.keep_arrived(saved);
        }

        return Ok(());
    }

    resume(env, &arrival, &kind, &folders).await
}

/// The session's folder in the source's checkout and in this machine's, and
/// the two checkouts themselves (for a chat's session, its folder twice). A
/// session works in its project folder,
/// never in a worktree: a record from an app that still sent a worktree
/// move resumes in the project folder.
fn folders(env: &Env, record: &SessionMove) -> Result<(Folders, Folders)> {
    if let Some(chat) = &record.conversation {
        // A chat's session keeps its place under the chats, which here are
        // in this machine's home folder.
        let new = conversation::moved(&env.home, &record.path)
            .context("the session's folder is not a chat's")?;
        let old = record
            .path
            .split('/')
            .fold(PathBuf::from(&chat.root), |folder, name| folder.join(name));
        let folders = Folders { old, new };
        return Ok((folders.clone(), folders));
    }

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
/// answers the name the source keeps the transcript under.
async fn conversation(
    env: &Env,
    record: &SessionMove,
    tracker: &Tracker,
    copy: &Path,
) -> Result<String> {
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

        match pull_once(env, record, copy, tracker).await {
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

                pulled_shown(tracker, &pulled);

                if !pulled.live {
                    return Ok(pulled.file_name);
                }

                // The source said it ended the session: read the rest now.
                if pulled.ended {
                    continue;
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

/// Tells that the source let a read through, so the session can move, and
/// how much came.
fn pulled_shown(tracker: &Tracker, pulled: &read::Pulled) {
    tracker.take();
    show(tracker, |progress| {
        progress.source_away = false;
        progress.copied = pulled.size;
        progress.total = pulled.size;
        if !pulled.live {
            progress.phase = MovePhase::Conversation;
        }
    });
}

/// One read of the transcript. While the source still runs the session, it
/// keeps the stream and says when it ended.
async fn pull_once(
    env: &Env,
    record: &SessionMove,
    copy: &Path,
    tracker: &Tracker,
) -> Result<read::Pulled> {
    with_stream(env, record.from, |tx, rx| {
        read::pull_following(
            tx,
            rx,
            read::Ask {
                agent: record.agent.clone(),
                id: &record.id,
                copy,
            },
            |pulled| pulled_shown(tracker, pulled),
        )
    })
    .await
}

/// Waits until this machine's checkout of the project is at the source's
/// commit, saying both while they differ, and answers what the source said.
/// The source is asked again when this machine's commit moves.
async fn checkouts(
    env: &Env,
    record: &SessionMove,
    tracker: &Tracker,
    root: &Path,
) -> Result<Checkout> {
    show(tracker, |progress| progress.phase = MovePhase::Files);
    let from = env.name_of(&record.from);
    let moved = Arc::new(Notify::new());
    let _watch = watch_head(root, moved.clone()).await?;
    let mut wait = FIRST_WAIT;
    // What the source said, and this machine's commit when it did.
    let mut said: Option<(Checkout, Option<String>)> = None;

    loop {
        let dir = root.to_owned();
        let ours = spawn_blocking(move || worktree::head(&dir)).await??;

        let theirs = match said.take() {
            Some((theirs, at)) if at == ours => theirs,
            _ => match ask_checkout(env, record).await {
                Ok(theirs) => theirs,
                Err(e) if e.is::<Refused>() => return Err(e),
                Err(e) => {
                    log::debug!(
                        "session {}: the source's checkout not read: {e:#}",
                        record.id
                    );
                    show(tracker, |progress| progress.source_away = true);
                    sleep(wait).await;
                    wait = (wait * 2).min(LAST_WAIT);
                    continue;
                }
            },
        };

        if theirs.head == ours {
            show(tracker, |progress| {
                progress.source_away = false;
                progress.reason = None;
                progress.edits_left = theirs.edits;
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
        said = Some((theirs, ours));
        moved.notified().await;
    }
}

/// Rings `moved` when the commit of the checkout at `root` may have changed.
/// A folder that is no repository yet is watched itself, for the repository
/// to appear in it.
async fn watch_head(root: &Path, moved: Arc<Notify>) -> Result<Paths> {
    let mut paths = watch_paths(root).await?;

    if paths.is_empty() {
        paths.push(root.to_owned());
    }

    let watch = Paths::new("move-checkouts", move |_| moved.notify_one())?;
    watch.replace(paths).await?;
    Ok(watch)
}

/// A commit as people read it, or that there is none.
fn short(head: Option<&str>) -> String {
    head.map_or_else(
        || "no commit".to_owned(),
        |head| head.chars().take(7).collect(),
    )
}

async fn ask_checkout(env: &Env, record: &SessionMove) -> Result<Checkout> {
    with_stream(env, record.from, |tx, rx| {
        read::checkout(tx, rx, &record.id)
    })
    .await
}

/// Brings each of the session's worktrees from the source and makes it
/// here, at the same folder in the project's checkout `root`. Each one
/// placed is kept in the move's state, to take back if the move does not
/// happen.
async fn worktrees(
    env: &Env,
    record: &SessionMove,
    tracker: &Tracker,
    root: &Path,
    checkout: Checkout,
) -> Result<()> {
    let total = u32::try_from(checkout.worktrees.len()).unwrap_or(u32::MAX);
    let base = checkout.head.unwrap_or_default();

    for (at, tree) in (1..).zip(checkout.worktrees) {
        let relative = Path::new(&tree.path);
        let inside = relative
            .components()
            .all(|part| matches!(part, Component::Normal(name) if name != ".git"));

        if !inside {
            bail!("the worktree {} is not inside the project", tree.path);
        }

        let name = relative.file_name().map_or_else(
            || tree.path.clone(),
            |name| name.to_string_lossy().into_owned(),
        );
        show(tracker, |progress| {
            progress.worktree = Some(name.clone());
            progress.worktree_at = at;
            progress.worktrees = total;
        });
        let (main, head) = (root.to_owned(), tree.head.clone());
        let bundle = !spawn_blocking(move || worktree::has_commit(&main, &head)).await?;
        let ask = read::WorktreeAsk {
            id: &record.id,
            path: &tree.path,
            base: &base,
            bundle,
        };
        let fetched = fetch_worktree(env, record, tracker, ask)
            .await
            .with_context(|| format!("bringing the worktree {name}"))?;
        let (main, path) = (root.to_owned(), root.join(relative));
        let (state, id, at) = (env.state.clone(), record.id.clone(), record.at);
        // Held until the worktree is noted, even if this task is stopped
        // meanwhile: taking the move back waits for it.
        let placing = env.agents.moves.placing(&record.id).lock_owned().await;
        spawn_blocking(move || {
            let _placing = placing;

            if let Some(bytes) = &fetched.bundle {
                worktree::import(&main, &fetched.work.head, bytes)?;
            }

            let was = worktree::tip(&main, &tree.branch);
            let placed = worktree::attach(&main, &path, &tree.branch, &fetched.work)?;
            let head = fetched.work.head;
            let tree = PlacedTree {
                applied: worktree::fingerprint(&path)?,
                moved: (was.as_deref() != Some(head.as_str())).then_some(worktree::Moved {
                    branch: tree.branch,
                    was,
                    to: head,
                }),
                main,
                path,
                placed,
            };

            if let Err(e) = keep_placed(&state, &id, at, tree.clone()) {
                // Not noted, so taken back now: nothing else would.
                let PlacedTree {
                    main,
                    path,
                    placed,
                    applied,
                    moved,
                } = &tree;
                let _ = worktree::take_back(main, path, placed, applied, moved.as_ref());
                return Err(e);
            }

            Ok(())
        })
        .await?
        .with_context(|| format!("making the worktree {name} here"))?;
        log::info!("session {}: the worktree {name} is here", record.id);
    }

    show(tracker, |progress| progress.worktree = None);
    Ok(())
}

/// Notes a worktree placed for the move of session `id` made at `at`.
fn keep_placed(state: &Folder, id: &SessionId, at: u64, placed: PlacedTree) -> Result<()> {
    let mut saved = state
        .load(id)
        .filter(|saved| saved.at == at)
        .unwrap_or_else(|| Saved::over(at, Phase::Working, None));
    saved.placed.push(placed);
    state
        .save(id, &saved)
        .context("keeping the worktrees placed for the move")
}

/// Takes back the worktrees placed for the move of session `id`, which did
/// not happen. One that changed since stays, and is logged.
pub(super) async fn take_back(env: &Env, id: &SessionId, at: u64) {
    // A worktree being placed is noted first.
    let placing = env.agents.moves.placing(id);
    let _placing = placing.lock().await;
    let placed = env
        .state
        .load(id)
        .filter(|saved| saved.at == at)
        .map(|saved| saved.placed)
        .unwrap_or_default();

    for tree in placed {
        let shown = tree.path.display().to_string();
        let done = spawn_blocking(move || {
            let moved = tree.moved.as_ref();
            worktree::take_back(&tree.main, &tree.path, &tree.placed, &tree.applied, moved)
        })
        .await;

        match done {
            Ok(Ok(())) => log::info!("took back the worktree {shown} made for a move"),
            Ok(Err(e)) => log::warn!("the worktree {shown} made for a move stays: {e:#}"),
            Err(e) => log::warn!("taking back the worktree {shown} failed: {e}"),
        }
    }
}

/// What the source saved of a chat's session, and the downloaded file archive.
async fn chat_files(
    env: &Env,
    record: &SessionMove,
    tracker: &Tracker,
) -> Result<(Option<SavedSession>, PathBuf)> {
    show(tracker, |progress| progress.phase = MovePhase::Files);
    let archive = env.state.copy(&record.id).with_file_name("files.tar");
    let saved = from_source(record, tracker, "its files", || async {
        let checkout = ask_checkout(env, record).await?;
        show(tracker, |progress| {
            (progress.files_copied, progress.files_total) = (0, checkout.files)
        });
        // Chunks come hundreds a second: each percent, or a quarter second, is told.
        let (step, mut told) = ((checkout.files / 100).max(1), (Instant::now(), 0));
        let came = move |held: u64| {
            if held.saturating_sub(told.1) >= step || told.0.elapsed() >= TELL_EVERY {
                told = (Instant::now(), held);
                show(tracker, |progress| progress.files_copied = held);
            }
        };
        with_stream(env, record.from, |tx, rx| {
            read::files(tx, rx, &record.id, &archive, came)
        })
        .await
        .map(|()| checkout.session)
    })
    .await?;
    Ok((saved, archive))
}

/// One worktree from the source.
async fn fetch_worktree(
    env: &Env,
    record: &SessionMove,
    tracker: &Tracker,
    ask: read::WorktreeAsk<'_>,
) -> Result<read::Fetched> {
    from_source(record, tracker, "a worktree", || {
        with_stream(env, record.from, |tx, rx| read::worktree(tx, rx, &ask))
    })
    .await
}

/// What `ask` brings from the source, asked again while the source cannot
/// be reached or is not done with the session; a refusal ends it.
async fn from_source<T, F: Future<Output = Result<T>>>(
    record: &SessionMove,
    tracker: &Tracker,
    what: &str,
    ask: impl Fn() -> F,
) -> Result<T> {
    let mut wait = FIRST_WAIT;

    loop {
        match ask().await {
            Ok(brought) => {
                show(tracker, |progress| progress.source_away = false);
                return Ok(brought);
            }
            Err(e) if e.is::<Refused>() => return Err(e),
            Err(e) => {
                log::warn!(
                    "session {}: {what} not read yet, asking again in {wait:?}: {e:#}",
                    record.id
                );
                show(tracker, |progress| progress.source_away = true);
                sleep(wait).await;
                wait = (wait * 2).min(LAST_WAIT);
            }
        }
    }
}
