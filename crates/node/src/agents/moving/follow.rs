//! Takes part in the moves the mesh's `session-move/<id>` records ask for:
//! as the source of a session, or as its target. Each move is one task,
//! owned here; a restart starts them again from the records and what
//! `saved` keeps.

use super::{
    Env, MAX_MOVES,
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
use tokio_util::task::AbortOnDropHandle;

/// What this node does for one record.
struct Running {
    at: u64,
    target: bool,
    task: AbortOnDropHandle<()>,
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

async fn reconcile(env: &Arc<Env>, running: &mut HashMap<String, Running>) {
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
        // Past the resume the session runs here: nothing to take back.
        if one.task.is_finished() || env.agents.moves.committed(&id) {
            running.remove(&id);
            continue;
        }
        if let Some(one) = running.remove(&id) {
            drop(one.task);
            cancelled(env, &id, one.target).await;
        }
    }
    for (id, record) in records {
        if running.get(&id).is_some_and(|one| one.at == record.at) {
            continue;
        }
        if running.len() >= MAX_MOVES {
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
        let result = if target {
            let tracker = env.agents.moves.begin(&record.id);
            target::run(&env, &record, &tracker).await
        } else {
            source::run(&env, &record).await
        };
        if let Err(e) = result {
            log::warn!("the move of session {} did not finish: {e:#}", record.id);
        }
    }))
}

async fn cancelled(env: &Env, id: &str, target: bool) {
    if target {
        env.agents.moves.begin(id).send_modify(|progress| {
            progress.phase = MovePhase::Cancelled;
        });
    } else {
        source::cancel(env, id).await;
    }
    env.state.clear(id, true);
    log::info!("the move of session {id} was cancelled");
}
