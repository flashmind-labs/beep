//! Takes part in the moves the mesh's `session-move/<id>` records ask for:
//! as the source of a session, or as its target. Each move is one task,
//! owned here; a restart starts them again from the records and what
//! `saved` keeps.

use super::{
    Env, MAX_MOVES,
    dial::watch_target,
    progress::Target,
    saved::{Folder, valid_id},
    source,
    store::StoreRoots,
    target,
};
use crate::agents::Agents;
use beep_mesh::{Key, Mesh};
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
    target: bool,
    /// The machine the session moves to.
    to: EndpointId,
    task: AbortOnDropHandle<()>,
}

impl Running {
    /// The move is past the point where it can be taken back: this machine
    /// is the target, and the session starts or runs here.
    fn committed(&self, env: &Env, id: &str) -> bool {
        self.target && env.agents.moves.committed(id)
    }
}

/// Whether another move fits: those that ended do not take room.
fn has_room(running: &HashMap<String, Running>) -> bool {
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
pub(crate) async fn follow(
    mesh: Mesh,
    endpoint: Endpoint,
    dir: PathBuf,
    agents: Arc<Agents>,
) {
    let old = dir.clone();
    let _ = tokio::task::spawn_blocking(move || drop_copies(&old)).await;
    let Some(roots) = StoreRoots::locate() else {
        log::warn!("not moving sessions: no home folder to find the agents' stores in");
        return;
    };
    let env = Arc::new(Env {
        agents,
        mesh,
        endpoint,
        state: Folder::new(&dir),
        roots,
    });
    let mut updates = env.mesh.updates();
    let mut running: HashMap<String, Running> = HashMap::new();

    loop {
        reconcile(&env, &mut running).await;

        if updates.changed().await.is_err() {
            return;
        }
    }
}

/// The records this machine takes part in, by session.
fn records(env: &Env) -> HashMap<String, SessionMove> {
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
            (mine && record.id == id && valid_id(&id)).then_some((id, record))
        })
        .collect()
}

pub(super) async fn reconcile(env: &Arc<Env>, running: &mut HashMap<String, Running>) {
    let records = records(env);
    let gone: Vec<String> = running
        .iter()
        .filter(|(id, _)| !records.contains_key(*id))
        .map(|(id, _)| id.clone())
        .collect();
    for id in gone {
        let Some(one) = running.get(&id) else {
            continue;
        };
        if one.task.is_finished() {
            running.remove(&id);
        } else if !one.committed(env, &id)
            && let Some(mut one) = running.remove(&id)
        {
            // Its task stops first, so nothing it does races the take-back.
            one.task.abort();
            let _ = (&mut one.task).await;
            cancelled(env, &id, &one).await;
        }
        // Past the resume the session runs here: nothing to take back, and
        // the task is left to finish starting it.
    }

    for (id, record) in records {
        if running.get(&id).is_some_and(|one| one.at == record.at) {
            continue;
        }

        if !has_room(running) {
            log::warn!("not taking part in the move of session {id}: {MAX_MOVES} are going");
            continue;
        }

        let target = record.to == env.mesh.me();
        let task = spawn(env.clone(), record.clone(), target);
        running.insert(
            id,
            Running {
                at: record.at,
                target,
                to: record.to,
                task,
            },
        );
    }
}

fn spawn(env: Arc<Env>, record: SessionMove, target: bool) -> AbortOnDropHandle<()> {
    AbortOnDropHandle::new(tokio::spawn(async move {
        log::info!(
            "taking part in the move of {} session {}, as its {}",
            record.agent,
            record.id,
            if target { "target" } else { "source" }
        );
        let result = if target {
            let tracker = env.agents.moves.begin(&record.id, record.at);
            target::run(&env, &record, &tracker).await
        } else {
            // What this machine knew of the session's earlier moves does not
            // answer for this one.
            env.agents.moves.forget(&record.id);
            let (said, mut target) = watch::channel(Target::Unknown);
            tokio::select! {
                done = source::run(&env, &record, &mut target) => done,
                () = watch_target(&env, &record, said) => Ok(()),
            }
        };

        match result {
            Ok(()) => log::info!("the move of session {} is done on this side", record.id),
            Err(e) => log::warn!("the move of session {} did not finish: {e:#}", record.id),
        }
    }))
}

async fn cancelled(env: &Env, id: &str, one: &Running) {
    if one.target {
        target::take_back(env, id).await;
        env.agents.moves.begin(id, one.at).send_modify(|progress| {
            progress.phase = MovePhase::Cancelled;
        });
    } else {
        source::cancel(env, id, one.at, one.to).await;
    }

    env.state.clear(id, true);
    log::info!("the move of session {id} was cancelled");
}

#[cfg(test)]
#[path = "../../tests/agents_moving_follow.rs"]
mod tests;
