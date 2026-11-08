//! Project preferences and status for separately synced ignored directories.

use super::*;
use beep_proto::node::sync::IgnoredSync;

impl Desk {
    pub async fn project_ignored_sync(
        &self,
        project: &str,
        settings: Option<IgnoredSync>,
    ) -> Result<IgnoredSync> {
        let _write = WRITES.lock().await;
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
mod tests {
    use super::*;
    use beep_proto::node::sync::ignored_project;

    #[test]
    fn ignored_content_is_counted_and_conflicts_keep_the_selected_folder_prefix() {
        let main = ProjectStatus {
            name: "acme/widget".into(),
            state: State::Idle,
            git_only: true,
            ..ProjectStatus::default()
        };
        let config = IgnoredSync {
            enabled: true,
            folders: vec!["models".into()],
        };
        let child = ProjectStatus {
            name: ignored_project(&main.name, "models"),
            state: State::Idle,
            files: 3,
            bytes: 700,
            conflicts: vec!["scene (conflict abcdef01).blend".into()],
            ..ProjectStatus::default()
        };
        let status = config.status(&main, &[child]);
        assert_eq!(status.files, 3);
        assert_eq!(status.conflicts, ["models/scene (conflict abcdef01).blend"]);
        assert!(status.git_only);
        assert_eq!(config.status(&main, &[]).state, State::Failed);
    }
}
