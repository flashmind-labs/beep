//! Project imports backed by the mesh's project and checkout records.

use crate::records::{
    Place, ProjectRecord, Stored, WRITES, at, find_project, now_ms, place_key, project_key,
    projects, read, sync_checkouts, write,
};
use async_trait::async_trait;
use beep_mesh::Mesh;
use beep_projects::{
    imports::{
        records::{ImportRow, NewImport},
        store::ImportStore,
    },
    model::RepoName,
};
use iroh::EndpointId;
use std::str::FromStr;

pub(super) fn row(stored: &Stored, machine: EndpointId, place: &Place) -> ImportRow {
    ImportRow {
        project: stored.name.clone(),
        endpoint: machine.to_string(),
        folder: place.folder.clone(),
        url: stored.record.url.clone(),
        imported_at: at(place.imported_ms),
        fork: place
            .fork
            .as_deref()
            .and_then(|fork| RepoName::from_str(fork).ok()),
    }
}

/// The projects as the import code reads and writes them: `project/<name>` and one
/// `project/<name>/<machine>` per checkout in the mesh.
pub(crate) struct MeshStore {
    mesh: Mesh,
}

impl MeshStore {
    pub(crate) fn new(mesh: &Mesh) -> Self {
        Self { mesh: mesh.clone() }
    }

    pub(crate) fn rows(&self) -> Vec<ImportRow> {
        let stored = projects(&self.mesh);
        stored
            .iter()
            .flat_map(|stored| {
                stored
                    .places
                    .iter()
                    .map(|(machine, place)| row(stored, *machine, place))
            })
            .collect()
    }

    /// Makes the project's own key if it is not there, with `url`.
    async fn ensure(&self, name: &str, url: &str) -> anyhow::Result<()> {
        let key = project_key(name);
        let mut record = read::<ProjectRecord>(&self.mesh, &key).unwrap_or_default();

        if record.url.is_empty() {
            record.url = url.to_owned();
            write(&self.mesh, key, Some(&record)).await?;
        }

        Ok(())
    }

    /// The name the project is stored under, which is the one it was first imported as, in any
    /// case it is asked for; the name given when it is new.
    fn name_of_repo(&self, project: &str, url: &str) -> String {
        projects(&self.mesh)
            .into_iter()
            .find(|stored| {
                std::iter::once(&stored.record.url)
                    .chain(
                        stored
                            .record
                            .remote_renames
                            .iter()
                            .flat_map(|rename| [&rename.from, &rename.to]),
                    )
                    .any(|known| {
                        beep_projects::imports::repo::remote_key(known).is_some_and(|known| {
                            Some(known) == beep_projects::imports::repo::remote_key(url)
                        })
                    })
            })
            .or_else(|| find_project(&self.mesh, project))
            .map_or_else(|| project.to_owned(), |stored| stored.name)
    }

    /// A record that is not written leaves the import standing; the next session writes it.
    async fn sync_project(&self, name: &str) {
        let Some(stored) = find_project(&self.mesh, name) else {
            return;
        };

        if let Err(e) = sync_checkouts(&self.mesh, &stored).await {
            log::warn!("{name} does not sync yet: {e}");
        }
    }

    fn stored_row(&self, name: &str, machine: EndpointId) -> anyhow::Result<ImportRow> {
        let stored = find_project(&self.mesh, name).ok_or_else(|| anyhow::anyhow!("not stored"))?;
        let place = stored
            .places
            .get(&machine)
            .ok_or_else(|| anyhow::anyhow!("not stored"))?;
        Ok(row(&stored, machine, place))
    }
}

#[async_trait]
impl ImportStore for MeshStore {
    async fn imports(&self) -> anyhow::Result<Vec<ImportRow>> {
        Ok(self.rows())
    }

    async fn save_import(&self, import: NewImport<'_>) -> anyhow::Result<ImportRow> {
        let machine = import.endpoint.parse::<EndpointId>()?;
        let _write = WRITES.lock().await;
        let name = self.name_of_repo(import.project, import.url);
        self.ensure(&name, import.url).await?;
        let key = place_key(&name, machine);
        let old = read::<Place>(&self.mesh, &key);
        let place = Place {
            folder: import.folder.to_owned(),
            // Moving a checkout keeps when it was first imported.
            imported_ms: old.as_ref().map_or_else(now_ms, |old| old.imported_ms),
            fork: old.and_then(|old| old.fork),
        };
        write(&self.mesh, key, Some(&place)).await?;
        self.sync_project(&name).await;
        self.stored_row(&name, machine)
    }

    async fn link_import(&self, import: NewImport<'_>) -> anyhow::Result<bool> {
        let machine = import.endpoint.parse::<EndpointId>()?;
        let _write = WRITES.lock().await;
        let name = self.name_of_repo(import.project, import.url);
        let key = place_key(&name, machine);

        if self.mesh.get(&key).is_some() {
            return Ok(false);
        }

        self.ensure(&name, import.url).await?;
        let place = Place {
            folder: import.folder.to_owned(),
            imported_ms: now_ms(),
            fork: None,
        };
        write(&self.mesh, key, Some(&place)).await?;
        self.sync_project(&name).await;
        Ok(true)
    }

    async fn set_import_fork(
        &self,
        project: &str,
        endpoint: &str,
        fork: Option<&RepoName>,
    ) -> anyhow::Result<()> {
        let machine = endpoint.parse::<EndpointId>()?;
        let _write = WRITES.lock().await;
        let key = place_key(
            &find_project(&self.mesh, project)
                .map_or_else(|| project.to_owned(), |stored| stored.name),
            machine,
        );
        let Some(mut place) = read::<Place>(&self.mesh, &key) else {
            return Ok(());
        };
        let fork = fork.map(ToString::to_string);

        if place.fork == fork {
            return Ok(());
        }

        place.fork = fork;
        write(&self.mesh, key, Some(&place)).await?;
        Ok(())
    }
}
