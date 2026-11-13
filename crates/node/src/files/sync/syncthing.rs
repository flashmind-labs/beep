//! Hands one checkout between Beep and Syncthing without pausing its share.

use super::{Held, Syncs, halt, lock, open_target, spawn_blocking};
use anyhow::Result;
use beep_proto::node::sync::SyncthingFiles;

impl Syncs {
    pub async fn syncthing_files(
        &self,
        folder: &str,
        enabled: Option<bool>,
    ) -> Result<SyncthingFiles> {
        let folders = if self.follows_mesh() {
            self.inner.folders.everywhere()
        } else {
            self.inner.folders.clone()
        };
        let path = folder.to_owned();
        let root = spawn_blocking(move || open_target(&folders, &path)).await??;

        if enabled.is_none() {
            return self.inner.syncthing.project_files(root.path(), None).await;
        }

        let _one = self.inner.configuring.lock().await;
        // Stop Beep before allowing Syncthing to write this checkout again.
        let names: Vec<_> = if enabled == Some(true) {
            lock(&self.inner.projects)
                .iter()
                .filter(|(_, running)| running.shared.root().starts_with(root.path()))
                .map(|(name, _)| name.clone())
                .collect()
        } else {
            Vec::new()
        };
        let mut stopped = Vec::new();

        for name in names {
            let running = lock(&self.inner.projects).remove(&name);

            if let Some(running) = running {
                let project = running.project.clone();
                let from = running.from;
                halt(running).await;
                stopped.push((project, from));
            }
        }

        let result = self
            .inner
            .syncthing
            .project_files(root.path(), enabled)
            .await;
        for (project, from) in stopped {
            if let Some(managed) = self
                .inner
                .syncthing
                .check(std::path::Path::new(&project.folder))
                .await
            {
                lock(&self.inner.held).insert(
                    project.name.clone(),
                    Held {
                        project,
                        from,
                        managed,
                    },
                );
            } else {
                match self.run(&project, from).await {
                    Ok(running) => {
                        lock(&self.inner.projects).insert(project.name, running);
                    }
                    Err(e) => log::warn!("could not resume project sync: {e:#}"),
                }
            }
        }

        drop(_one);

        self.check_syncthing().await;

        result
    }
}
