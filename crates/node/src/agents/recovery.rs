//! Letting Git recovery run in a checkout. Sync publishes the checkouts
//! whose local edits wait for it; this follows them, checks that nothing
//! runs there and answers each with the hold that lets sync recover.

use super::{Agents, MAX_IN_FOLDERS};
use crate::files::{holds::RepositoryHold, sync::Syncs};
use anyhow::{Result, ensure};
use beep_proto::node::agent::Presence;
use std::{path::Path, sync::Arc};
use tokio_util::task::AbortOnDropHandle;

impl Agents {
    /// Answers the checkouts `syncs` publishes as waiting for recovery, for
    /// as long as the agents live. Calling it again changes nothing.
    pub fn follow_recoveries(self: &Arc<Self>, syncs: &Syncs) {
        self.recovering.get_or_init(|| {
            let agents = Arc::downgrade(self);
            let syncs = syncs.clone();
            let mut asks = syncs.recoveries();

            AbortOnDropHandle::new(tokio::spawn(async move {
                loop {
                    let roots: Vec<_> = beep_proto::watched::read_and_update(&mut asks, |roots| roots.iter().cloned().collect());
                    let Some(held) = agents.upgrade() else {
                        return;
                    };
                    futures::future::join_all(roots.iter().map(|root| async {
                        let answer = held.reserve_repository(root).await;
                        syncs.answer_recovery(root, answer);
                    }))
                    .await;
                    drop(held);

                    if asks.changed().await.is_err() {
                        return;
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
        let root = folder.to_owned();
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
