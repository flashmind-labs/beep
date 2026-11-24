//! Project preferences kept in `sync/<name>`: separately synced ignored
//! directories and the project's sync rules.

use super::*;
use beep_proto::node::sync::IgnoredSync;

/// The longest sync rules text a project keeps.
const MAX_RULES: usize = 16 * 1024;

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
        let (key, mut spec) = self.sync_spec(project)?;

        if let Some(settings) = settings {
            spec.ignored = settings.normalized().map_err(FleetError::refused)?;
            write(&self.mesh, key, Some(&spec)).await?;
        }

        Ok(spec.ignored)
    }

    /// The project's sync rules, in `.gitignore` syntax, which every machine
    /// applies after its `.beepignore`. `rules` replaces them.
    pub async fn project_sync_rules(&self, project: &str, rules: Option<String>) -> Result<String> {
        let _write = match rules {
            Some(_) => Some(WRITES.lock().await),
            None => None,
        };
        let (key, mut spec) = self.sync_spec(project)?;

        if let Some(rules) = rules {
            let rules = rules.replace("\r\n", "\n").trim().to_owned();
            if rules.len() > MAX_RULES {
                return Err(FleetError::refused("Keep the sync rules under 16 KB."));
            }

            spec.rules = rules;
            write(&self.mesh, key, Some(&spec)).await?;
        }

        Ok(spec.rules)
    }

    /// The project's `sync/<name>` record, or a new one for its machines.
    fn sync_spec(&self, project: &str) -> Result<(Key, SyncSpec)> {
        let stored = find_project(&self.mesh, project).ok_or(FleetError::NotFound)?;
        let key = Key::Sync(stored.name.clone());
        let spec = read::<SyncSpec>(&self.mesh, &key).unwrap_or_else(|| SyncSpec {
            folders: stored
                .places
                .iter()
                .filter(|(id, _)| self.mesh.is_member(**id))
                .map(|(id, place)| (*id, place.folder.clone()))
                .collect(),
            ..SyncSpec::default()
        });
        Ok((key, spec))
    }
}

#[cfg(test)]
#[path = "../tests/sync_ignored.rs"]
mod tests;
