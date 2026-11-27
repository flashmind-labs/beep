//! Hands one checkout between Beep and Syncthing without pausing its share.

use super::{Phase, Syncs, halt, open_target, spawn_blocking};
use anyhow::{Result, bail};
use beep_proto::node::sync::SyncthingFiles;
use futures::future::join_all;
use std::path::Path;

impl Syncs {
    pub async fn syncthing_files(
        &self,
        folder: &str,
        enabled: Option<bool>,
    ) -> Result<SyncthingFiles> {
        let folders = if self.inner.mesh.get().is_some() {
            self.inner.folders.everywhere()
        } else {
            self.inner.folders.clone()
        };
        let path = folder.to_owned();
        let root = spawn_blocking(move || open_target(&folders, &path)).await??;

        if enabled.is_none() {
            return self.inner.syncthing.project_files(root.path(), None).await;
        }

        let root = root.path().to_owned();
        let enabled = enabled == Some(true);

        self.change("syncthing_files", move |syncs| async move {
            syncs.do_syncthing_files(&root, enabled).await
        })
        .await
        .unwrap_or_else(|why| bail!(why))
    }

    /// Stops the projects in `root` before Syncthing may write it, lets it,
    /// and starts them again unless Syncthing keeps the folder.
    pub(super) async fn do_syncthing_files(
        &self,
        root: &Path,
        enabled: bool,
    ) -> Result<SyncthingFiles> {
        let state = &self.inner.projects;
        let result = {
            // One change at a time on a folder, then on each project in it.
            let _root = state.gate(&format!("syncthing {}", root.display())).await;
            // Stop Beep before allowing Syncthing to write this checkout again.
            let names: Vec<String> = if enabled {
                state
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
            let _gates = state.gates(names.clone()).await;
            let mut taken = Vec::new();

            for name in names {
                let entry = state.seen().get(&name).cloned();

                if let (Some(running), Some(entry)) = (state.take(&name), entry) {
                    taken.push((entry, running));
                }
            }

            let stopped = join_all(taken.into_iter().map(|(entry, running)| async move {
                (entry.project, entry.from, halt(running).await)
            }))
            .await;
            let result = self
                .inner
                .syncthing
                .project_files(root, Some(enabled))
                .await;

            join_all(
                stopped
                    .into_iter()
                    .map(|(project, from, shared)| async move {
                        if let Some(managed) =
                            self.inner.syncthing.check(Path::new(&project.folder)).await
                        {
                            // Syncthing writes the folder now: start from a fresh scan.
                            self.forget(&shared).await;
                            state.put(&project, from, Phase::Held(managed.reason()));
                        } else {
                            match self.run(&project, from).await {
                                Ok(running) => self.started(&project, from, running),
                                Err(e) => {
                                    let message = format!("{e:#}");
                                    log::warn!("could not resume project sync: {message}");
                                    state.put(&project, from, Phase::Stopped(message));
                                }
                            }
                        }
                    }),
            )
            .await;

            result
        };

        self.do_check_syncthing().await;
        result
    }
}
