//! Project names are labels. Repository URLs may change without moving mesh keys.

use crate::{
    desk::Desk,
    error::{FleetError, Result},
    records::{WRITES, find_project, project_key, projects, write},
};
use beep_projects::{
    imports::{
        hosts::{forgejo_of, rename_repository},
        repo::RepoRef,
    },
    model::RepoName,
};
use beep_proto::node::mesh_state::RemoteRename;
use std::str::FromStr;

impl Desk {
    /// Changes the displayed name and, when requested, the hosted repository name.
    pub async fn rename_project(
        &self,
        project: &str,
        name: &str,
        repository: Option<&str>,
    ) -> Result<()> {
        let name = name.trim();

        if name.is_empty() || name.chars().count() > 200 || name.chars().any(char::is_control) {
            return Err(FleetError::refused(
                "Use a project name of 1 to 200 characters without control characters.",
            ));
        }

        let _write = WRITES.lock().await;
        let mut stored = find_project(&self.mesh, project).ok_or(FleetError::NotFound)?;

        if projects(&self.mesh).iter().any(|other| {
            other.name != stored.name
                && (other.name.eq_ignore_ascii_case(name)
                    || other
                        .record
                        .display_name
                        .as_deref()
                        .unwrap_or(&other.name)
                        .eq_ignore_ascii_case(name))
        }) {
            return Err(FleetError::refused(
                "Another project already uses that name.",
            ));
        }

        let original_url = stored.record.url.clone();

        if let Some(repository) = repository {
            let credentials = self.opened();
            let repo = RepoRef::parse(&stored.record.url, &forgejo_of(&credentials))?;
            let renamed = RepoName::from_str(&format!("{}/{}", repo.org(), repository.trim()))
                .map_err(|e| FleetError::refused(e.to_string()))?;

            if renamed.repo() != repo.name() {
                let new = rename_repository(
                    &self.hosts,
                    &credentials,
                    &repo,
                    &renamed,
                    self.github_api.as_deref(),
                )
                .await?;
                stored.record.remote_renames.push(RemoteRename {
                    from: stored.record.url.clone(),
                    to: new.web_url(),
                });
                stored.record.url = new.web_url();
            }
        }

        stored.record.display_name = (name != stored.name).then(|| name.to_owned());
        if let Err(error) = write(&self.mesh, project_key(&stored.name), Some(&stored.record)).await
        {
            if stored.record.url != original_url {
                return Err(FleetError::refused(format!(
                    "The repository was renamed to {}, but saving the project failed: {error}",
                    stored.record.url
                )));
            }

            return Err(error);
        }
        self.hosts.forget_remotes(&stored.name);
        Ok(())
    }
}
