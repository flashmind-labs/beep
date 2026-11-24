//! Hands one checkout between Beep and Syncthing without pausing its share.

use super::{Op, Phase, Syncs, halt, lock, open_target, spawn_blocking};
use anyhow::{Result, bail};
use std::path::Path;
use beep_proto::node::sync::SyncthingFiles;

impl Syncs {
    pub async fn syncthing_files(
        &self,
        folder: &str,
        enabled: Option<bool>,
    ) -> Result<SyncthingFiles> {
        let folders = if lock(&self.inner.mesh).is_some() {
            self.inner.folders.everywhere()
        } else {
            self.inner.folders.clone()
        };
        let path = folder.to_owned();
        let root = spawn_blocking(move || open_target(&folders, &path)).await??;

        if enabled.is_none() {
            return self.inner.syncthing.project_files(root.path(), None).await;
        }

        let done = self
            .inner
            .actor
            .ask(|done| Op::SyncthingFiles {
                root: root.path().to_owned(),
                enabled: enabled == Some(true),
                done,
            })
            .await;
        done.unwrap_or_else(|why| bail!(why))
    }

    /// Stops the projects in `root` before Syncthing may write it, lets it,
    /// and starts them again unless Syncthing keeps the folder.
    pub(super) async fn do_syncthing_files(
        &self,
        root: &Path,
        enabled: bool,
    ) -> Result<SyncthingFiles> {
        let actor = &self.inner.actor;
        // Stop Beep before allowing Syncthing to write this checkout again.
        let names: Vec<_> = if enabled {
            actor
                .seen()
                .iter()
                .filter(|(_, entry)| {
                    entry
                        .running()
                        .is_some_and(|shared| shared.root().starts_with(root))
                })
                .map(|(name, _)| name.clone())
                .collect()
        } else {
            Vec::new()
        };
        let mut stopped = Vec::new();

        for name in names {
            let entry = actor.seen().get(&name).cloned();

            if let (Some(running), Some(entry)) = (actor.take(&name).await, entry) {
                let shared = halt(running).await;
                stopped.push((entry.project, entry.from, shared));
            }
        }

        let result = self
            .inner
            .syncthing
            .project_files(root, Some(enabled))
            .await;

        for (project, from, shared) in stopped {
            if let Some(managed) = self
                .inner
                .syncthing
                .check(Path::new(&project.folder))
                .await
            {
                // Syncthing writes the folder now: start from a fresh scan.
                self.forget(&shared).await;
                actor
                    .put(&project, from, Phase::Held(managed.reason()))
                    .await;
            } else {
                match self.run(&project, from).await {
                    Ok(running) => actor.start_run(&project, from, running).await,
                    Err(e) => {
                        let message = format!("{e:#}");
                        log::warn!("could not resume project sync: {message}");
                        actor.put(&project, from, Phase::Stopped(message)).await;
                    }
                }
            }
        }

        self.do_check_syncthing().await;
        result
    }
}
