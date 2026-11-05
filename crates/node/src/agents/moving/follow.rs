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
use crate::{agents::Agents, files::sync::Syncs};
use beep_mesh::{Key, Mesh};
use beep_proto::node::{
    agent::MovePhase,
    mesh_state::{SessionMove, StateValue},
};
use iroh::Endpoint;
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::sync::watch;
use tokio_util::task::AbortOnDropHandle;

/// What this node does for one record.
pub(super) struct Running {
    at: u64,
    target: bool,
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

/// Follows the mesh's move records until the node stops.
pub(crate) async fn follow(
    mesh: Mesh,
    endpoint: Endpoint,
    dir: PathBuf,
    agents: Arc<Agents>,
    syncs: Option<Syncs>,
) {
    let Some(roots) = StoreRoots::locate() else {
        log::warn!("not moving sessions: no home folder to find the agents' stores in");
        return;
    };
    let env = Arc::new(Env {
        agents,
        mesh,
        endpoint,
        syncs,
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
        } else if !one.committed(env, &id) {
            if let Some(one) = running.remove(&id) {
                drop(one.task);
                cancelled(env, &id, one.at, one.target).await;
            }
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

async fn cancelled(env: &Env, id: &str, at: u64, target: bool) {
    if target {
        env.agents.moves.begin(id, at).send_modify(|progress| {
            progress.phase = MovePhase::Cancelled;
        });
    } else {
        source::cancel(env, id).await;
    }
    env.state.clear(id, true);
    log::info!("the move of session {id} was cancelled");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::pending;

    async fn ended() -> Running {
        let task = AbortOnDropHandle::new(tokio::spawn(async {}));
        while !task.is_finished() {
            tokio::task::yield_now().await;
        }
        Running {
            at: 1,
            target: true,
            task,
        }
    }

    fn going() -> Running {
        Running {
            at: 1,
            target: false,
            task: AbortOnDropHandle::new(tokio::spawn(pending::<()>())),
        }
    }

    #[tokio::test]
    async fn moves_that_ended_leave_room_for_the_next() {
        let mut running = HashMap::new();
        for n in 0..MAX_MOVES + 4 {
            running.insert(format!("done{n}"), ended().await);
        }
        assert!(
            has_room(&running),
            "records of finished moves stay in the mesh"
        );
        let mut busy = HashMap::new();
        for n in 0..MAX_MOVES {
            busy.insert(format!("busy{n}"), going());
        }
        assert!(!has_room(&busy));
        busy.extend(running);
        assert!(!has_room(&busy), "only those that go count");
    }
}
