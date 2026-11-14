//! Reserving an idle checkout while Git preserves and restores local edits.

use super::{Agents, MAX_IN_FOLDERS};
use anyhow::{Result, ensure};
use beep_proto::node::agent::{Presence, Status};
use beep_proto::watched;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub(crate) struct RepositoryHold {
    agents: Arc<Agents>,
    folder: PathBuf,
}

impl Drop for RepositoryHold {
    fn drop(&mut self) {
        crate::lock(&self.agents.recovering).remove(&self.folder);
    }
}

impl Agents {
    pub(crate) async fn reserve_repository(
        self: &Arc<Self>,
        folder: &Path,
    ) -> Result<RepositoryHold> {
        let folder = folder.to_owned();
        let folder =
            tokio::task::spawn_blocking(move || crate::files::paths::canonicalize(&folder))
                .await??;
        let running = {
            // Same lock order as start: no session can appear between this check and reservation.
            let mut recovering = crate::lock(&self.recovering);
            ensure!(
                !recovering.contains(&folder),
                "Git recovery already runs in this checkout."
            );
            let running = self
                .lock()
                .values()
                .filter_map(|session| {
                    watched::read(&session.live, |live| {
                        (live.status() != Status::Ended).then(|| PathBuf::from(&live.info.cwd))
                    })
                })
                .collect::<Vec<_>>();
            recovering.insert(folder.clone());
            running
        };
        let hold = RepositoryHold {
            agents: self.clone(),
            folder: folder.clone(),
        };
        let root = folder.clone();
        let busy = tokio::task::spawn_blocking(move || {
            running.iter().any(|path| {
                path.starts_with(&root)
                    || crate::files::paths::canonicalize(path)
                        .is_ok_and(|real| real.starts_with(&root))
            })
        })
        .await?;
        ensure!(
            !busy,
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

#[cfg(all(test, unix))]
#[path = "../tests/agents_repository_recovery.rs"]
mod tests;
