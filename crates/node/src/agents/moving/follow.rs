//! Takes part in the moves the mesh's `session-move/<id>` records ask for:
//! as the source of a session, or as its target. Each move is one task,
//! owned here; a restart starts them again from the records and what
//! `saved` keeps.

use super::{
    Env, MAX_MOVES,
    dial::watch_target,
    progress::{Target, Tracker},
    saved::{Folder, Phase, valid_id},
    source,
    store::StoreRoots,
    target,
};
use crate::agents::Agents;
use beep_mesh::{Key, Mesh};
use beep_proto::SessionId;
use beep_proto::node::{
    agent::MovePhase,
    mesh_state::{SessionMove, StateValue},
};
use iroh::{Endpoint, EndpointId};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::watch;
use tokio_util::task::AbortOnDropHandle;

/// What this node does for one record.
pub(super) struct Running {
    at: u64,
    side: Side,
    task: AbortOnDropHandle<()>,
}

#[derive(Clone)]
enum Side {
    Source { to: EndpointId },
    Target(Arc<Tracker>),
}

impl Side {
    fn request_cancel(&self) -> bool {
        match self {
            Self::Source { .. } => true,
            Self::Target(tracker) => tracker.request_cancel(),
        }
    }
}

/// Whether another move fits: those that ended do not take room.
fn has_room(running: &HashMap<SessionId, Running>) -> bool {
    running
        .values()
        .filter(|one| !one.task.is_finished())
        .count()
        < MAX_MOVES
}

/// Takes away the copies background copying left in `<state>/sessions/`,
/// and its notes; a version that lost to another machine's is the user's
/// and stays.
fn drop_copies(state_dir: &Path) {
    let Ok(agents) = fs::read_dir(state_dir.join("sessions")) else {
        return;
    };

    for agent in agents.flatten() {
        for session in fs::read_dir(agent.path()).into_iter().flatten().flatten() {
            for file in fs::read_dir(session.path()).into_iter().flatten().flatten() {
                if !file.file_name().to_string_lossy().starts_with("lost-") {
                    let _ = fs::remove_file(file.path());
                }
            }

            let _ = fs::remove_dir(session.path());
        }

        let _ = fs::remove_dir(agent.path());
    }

    let _ = fs::remove_dir(state_dir.join("sessions"));
}

/// Follows the mesh's move records until the node stops.
pub(crate) async fn follow(mesh: Mesh, endpoint: Endpoint, dir: PathBuf, agents: Arc<Agents>) {
    let old = dir.clone();
    let _ = tokio::task::spawn_blocking(move || drop_copies(&old)).await;
    let (Some(roots), Some(home)) = (StoreRoots::locate(), dirs::home_dir()) else {
        log::warn!("not moving sessions: no home folder to find the agents' stores in");
        return;
    };
    agents.moves.keep_in(Folder::new(&dir));
    let env = Arc::new(Env {
        agents,
        mesh,
        endpoint,
        state: Folder::new(&dir),
        roots,
        home,
    });
    let mut updates = env.mesh.updates();
    let mut running: HashMap<SessionId, Running> = HashMap::new();

    loop {
        reconcile(&env, &mut running).await;

        tokio::select! {
            changed = updates.changed() => {
                if changed.is_err() {
                    return;
                }
            }
            () = env.agents.moves.ended.notified() => {}
        }
    }
}

/// The records this machine takes part in, by session.
fn records(env: &Env) -> HashMap<SessionId, SessionMove> {
    let me = env.mesh.me();
    env.mesh
        .scan("session-move/")
        .into_iter()
        .filter_map(|(key, bytes)| {
            let Key::SessionMove(id) = key else {
                return None;
            };
            let record = match SessionMove::from_bytes(&bytes) {
                Ok(record) => record,
                Err(_) => {
                    log::warn!("ignored session-move/{id} in the mesh: it does not read");
                    return None;
                }
            };
            let mine = (record.from == me) != (record.to == me);
            (mine && record.id == id && valid_id(id.as_ref())).then_some((id, record))
        })
        .collect()
}

/// `records` as they are taken up when more move than at once: the oldest
/// first, and a chat before its threads, so the move the user watches is
/// not the one left waiting.
pub(super) fn in_order(records: HashMap<SessionId, SessionMove>) -> Vec<(SessionId, SessionMove)> {
    let mut records: Vec<(SessionId, SessionMove)> = records.into_iter().collect();
    records.sort_by(|(_, a), (_, b)| (a.at, &a.path).cmp(&(b.at, &b.path)));

    records
}

pub(super) async fn reconcile(env: &Arc<Env>, running: &mut HashMap<SessionId, Running>) {
    let wanted = records(env);
    let superseded: Vec<SessionId> = running
        .iter()
        .filter(|(id, one)| wanted.get(*id).is_none_or(|record| one.at != record.at))
        .map(|(id, _)| id.clone())
        .collect();
    for id in superseded {
        let Some(one) = running.get(&id) else {
            continue;
        };
        if one.task.is_finished() {
            running.remove(&id);
        } else if one.side.request_cancel()
            && let Some(mut one) = running.remove(&id)
        {
            // Its task stops first, so nothing it does races the take-back.
            one.task.abort();
            let _ = (&mut one.task).await;
            cancelled(env, &id, &one).await;
        }
        // A target that claimed settlement finishes before a replacement.
    }

    for (id, record) in in_order(records(env)) {
        if running.contains_key(&id) {
            continue;
        }

        if !has_room(running) {
            log::warn!("not taking part in the move of session {id}: {MAX_MOVES} are going");
            continue;
        }

        // A restart may find the previous generation's staged work before
        // it has any running task to cancel.
        if let Some(old) = env.state.load(&id)
            && old.at != record.at
            && old.phase == Phase::Working
        {
            let (work, session, at) = (env.clone(), id.clone(), old.at);
            env.agents.moves.clean(&id, async move {
                target::take_back(&work, &session, at).await;
                work.state.clear(&session, at);
            });
        }

        let side = if record.to == env.mesh.me() {
            Side::Target(env.agents.moves.begin(&id, record.at, record.from))
        } else {
            env.agents.moves.forget(&id);
            Side::Source { to: record.to }
        };
        let at = record.at;
        let before = env.agents.moves.cleaned(&id);
        let task = spawn(env.clone(), record, side.clone(), before);
        running.insert(id, Running { at, side, task });
    }
}

/// Runs the move of `record`, once `before`, the take-back of the session's
/// last move, is done.
fn spawn(
    env: Arc<Env>,
    record: SessionMove,
    side: Side,
    before: Option<impl Future<Output = ()> + Send + 'static>,
) -> AbortOnDropHandle<()> {
    AbortOnDropHandle::new(tokio::spawn(async move {
        if let Some(before) = before {
            before.await;
        }

        log::info!(
            "taking part in the move of {} session {}, as its {}",
            record.agent,
            record.id,
            if matches!(side, Side::Target(_)) {
                "target"
            } else {
                "source"
            }
        );
        let result = match side {
            Side::Target(tracker) => target::run(&env, &record, &tracker).await,
            Side::Source { .. } => {
                let (said, mut target) = watch::channel(Target::Unknown);
                tokio::select! {
                    done = source::run(&env, &record, &mut target) => done,
                    () = watch_target(&env, &record, said) => Ok(()),
                }
            }
        };

        match result {
            Ok(()) => log::info!("the move of session {} is done on this side", record.id),
            Err(e) => log::warn!("the move of session {} did not finish: {e:#}", record.id),
        }

        env.agents.moves.ended.notify_one();
    }))
}

async fn cancelled(env: &Arc<Env>, id: &SessionId, one: &Running) {
    match &one.side {
        Side::Target(tracker) => {
            // Git work: beside the loop, which has other moves to start.
            let (work, session, at, tracker) = (env.clone(), id.clone(), one.at, tracker.clone());
            env.agents.moves.clean(id, async move {
                target::take_back(&work, &session, at).await;
                work.state.clear(&session, at);
                tracker.finish(MovePhase::Cancelled, None);
            });
        }
        Side::Source { to } => match source::cancel(env, id, one.at, *to).await {
            source::Recovery::Released => env.state.clear(id, one.at),
            source::Recovery::TargetMayRun => return,
        },
    }

    log::info!("the move of session {id} was cancelled");
}

#[cfg(test)]
#[path = "../../tests/agents_moving_follow.rs"]
mod tests;
