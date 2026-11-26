//! Letting Git recovery run in a checkout. Sync publishes the checkouts
//! whose local edits wait for it; this follows them, checks that nothing
//! runs there and answers each with the hold that lets sync recover.

use super::{Agents, MAX_IN_FOLDERS};
use crate::files::{holds::RepositoryHold, sync::Syncs};
use anyhow::{Result, ensure};
use beep_proto::node::agent::Presence;
use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Weak},
};
use tokio::task::{Id, JoinSet};
use tokio_util::task::AbortOnDropHandle;

impl Agents {
    /// Answers the checkouts `syncs` publishes as waiting for recovery, for
    /// as long as the agents live. Calling it again changes nothing.
    pub fn follow_recoveries(self: &Arc<Self>, syncs: &Syncs) {
        self.recovering.get_or_init(|| {
            let agents = Arc::downgrade(self);
            let syncs = syncs.clone();

            AbortOnDropHandle::new(tokio::spawn(async move {
                // The follower starts again if it dies; it ends when the
                // agents or the syncs are gone.
                loop {
                    let run = AbortOnDropHandle::new(tokio::spawn(follow(
                        agents.clone(),
                        syncs.clone(),
                    )));

                    match run.await {
                        Ok(()) => return,
                        Err(e) => {
                            log::error!("the recovery follower stopped, starting again: {e}");
                            tokio::time::sleep(beep_mesh::RETRY).await;
                        }
                    }
                }
            }))
        });
    }

    /// Holds the checkout `folder` for recovery when no session runs in it,
    /// judged now: the sessions of this node, and the sessions started
    /// outside Beep as the stores show them.
    pub(super) async fn reserve_repository(&self, folder: &Path) -> Result<RepositoryHold> {
        let holds = self.holds();
        let hold = holds.hold(folder).await?;
        let folder = hold.folder().to_owned();
        let root = folder.clone();
        let running = {
            let holds = holds.clone();
            tokio::task::spawn_blocking(move || holds.running_under(&root)).await?
        };
        ensure!(
            !running,
            "Local edits block Git sync; close running sessions before automatic recovery."
        );
        // A presence that cannot be established is not evidence of an idle checkout.
        let sessions = self
            .sessions_in(
                vec![folder.to_string_lossy().into_owned()],
                u32::try_from(MAX_IN_FOLDERS)?,
            )
            .await?;
        ensure!(
            sessions.len() < MAX_IN_FOLDERS
                && sessions
                    .iter()
                    .all(|session| session.presence == Presence::Stopped),
            "Local edits block Git sync; an external session is running or its presence is unknown."
        );

        Ok(hold)
    }
}

/// One task per root asking, so a slow read of one never delays another.
/// A task that ends without answering answers the root as refused: recovery
/// never runs without a read.
async fn follow(agents: Weak<Agents>, syncs: Syncs) {
    let mut asks = syncs.recoveries();
    let mut tasks = JoinSet::new();
    let mut roots: HashMap<Id, PathBuf> = HashMap::new();
    let mut active = BTreeSet::new();

    loop {
        let asked: Vec<PathBuf> = beep_proto::watched::read_and_update(&mut asks, |asked| {
            asked.iter().cloned().collect()
        });

        let fresh: Vec<PathBuf> = asked
            .into_iter()
            .filter(|root| !active.contains(root))
            .collect();

        for root in fresh {
            let (agents, syncs, own) = (agents.clone(), syncs.clone(), root.clone());
            let task = tasks.spawn(async move {
                let answer = match agents.upgrade() {
                    Some(agents) => agents.reserve_repository(&own).await,
                    None => Err(anyhow::anyhow!("The node is stopping.")),
                };
                syncs.answer_recovery(&own, answer);
            });
            roots.insert(task.id(), root.clone());
            active.insert(root);
        }

        tokio::select! {
            changed = asks.changed() => {
                if changed.is_err() {
                    return;
                }
            }
            Some(done) = tasks.join_next_with_id() => {
                let id = match done {
                    Ok((id, ())) => id,
                    Err(e) => {
                        log::error!("a recovery check stopped: {e}");
                        let id = e.id();

                        if let Some(root) = roots.get(&id) {
                            syncs.answer_recovery(
                                root,
                                Err(anyhow::anyhow!("The check of the running sessions stopped; recovery will ask again.")),
                            );
                        }

                        id
                    }
                };

                if let Some(root) = roots.remove(&id) {
                    active.remove(&root);
                }
            }
        }

        if agents.strong_count() == 0 {
            return;
        }
    }
}
