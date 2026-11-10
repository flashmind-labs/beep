//! Project preferences and status for separately synced ignored directories.

use super::*;
use beep_proto::node::sync::IgnoredSync;

impl Desk {
    pub async fn project_ignored_sync(
        &self,
        project: &str,
        settings: Option<IgnoredSync>,
    ) -> Result<IgnoredSync> {
        // A read needs no lock: it never writes back what it read.
        let _write = match settings {
            Some(_) => Some(WRITES.lock().await),
            None => None,
        };
        let stored = find_project(&self.mesh, project).ok_or(FleetError::NotFound)?;
        let key = Key::Sync(stored.name.clone());
        let mut spec = read::<SyncSpec>(&self.mesh, &key).unwrap_or_else(|| SyncSpec {
            folders: stored
                .places
                .iter()
                .filter(|(id, _)| self.mesh.is_member(**id))
                .map(|(id, place)| (*id, place.folder.clone()))
                .collect(),
            ..SyncSpec::default()
        });

        if let Some(settings) = settings {
            spec.ignored = settings.normalized().map_err(FleetError::refused)?;
            write(&self.mesh, key, Some(&spec)).await?;
        }

        Ok(spec.ignored)
    }
}

#[cfg(test)]
#[path = "../tests/sync_ignored.rs"]
mod tests;
