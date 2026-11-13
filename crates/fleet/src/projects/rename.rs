//! Project labels and independent renames of hosted repository copies.

use crate::{
    AsyncMutex,
    desk::Desk,
    error::{FleetError, Result},
    records::{ProjectRecord, Stored, WRITES, find_project, project_key, projects, write},
};
use beep_projects::{
    imports::{
        ImportError,
        hosts::{forgejo_of, rename_repository},
        repo::RepoRef,
    },
    model::RepoName,
};
use beep_proto::{ForgejoUrl, node::mesh_state::RemoteRename};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

static RENAMES: AsyncMutex<()> = AsyncMutex::const_new(());
const MAX_REPOSITORIES: usize = 16;

/// One repository copy offered by the rename dialog.
#[derive(Debug, Clone, Serialize)]
pub struct RenameRepositoryTarget {
    pub url: String,
    pub host: String,
    pub name: String,
}

/// An explicitly selected repository and its requested short name.
#[derive(Debug, Clone, Deserialize)]
pub struct RepositoryRename {
    pub url: String,
    pub name: String,
}

/// Each host succeeds or fails independently; successful changes survive a retry.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RepositoryRenameResult {
    Renamed { from: String, to: String },
    Failed { url: String, message: String },
}

struct RenamePlan {
    asked: String,
    repo: RepoRef,
    name: RepoName,
}

impl Desk {
    /// Lists matching repository copies from checkout remotes and saved rename history.
    pub async fn rename_repositories(&self, project: &str) -> Result<Vec<RenameRepositoryTarget>> {
        let stored = find_project(&self.mesh, project).ok_or(FleetError::NotFound)?;
        Ok(self
            .rename_targets(&stored)
            .await
            .into_iter()
            .map(|repo| RenameRepositoryTarget {
                url: repo.web_url(),
                host: repo.host_name(),
                name: repo.name().to_owned(),
            })
            .collect())
    }

    async fn rename_targets(&self, stored: &Stored) -> Vec<RepoRef> {
        let servers = forgejo_of(&self.opened());
        let Ok(own) = parse_repository(&stored.record.url, &servers) else {
            return Vec::new();
        };
        let history: Vec<RepoRef> = stored
            .record
            .remote_renames
            .iter()
            .flat_map(|rename| [&rename.from, &rename.to])
            .filter_map(|url| parse_repository(url, &servers).ok())
            .collect();
        let mut repos = vec![own.clone()];

        let mut candidates: Vec<RepoRef> = history
            .iter()
            .map(RepoRef::web_url)
            .chain(
                self.checkout_remotes(stored, crate::runs::RemoteScope::All)
                    .await,
            )
            .filter_map(|url| parse_repository(&url, &servers).ok())
            .collect();
        // Prefer known copies and matching names before other remotes on a new host.
        candidates.sort_by_key(|repo| {
            !(repo.name().eq_ignore_ascii_case(own.name()) || history.iter().any(|old| old == repo))
        });
        for repo in candidates {
            let url = current_url(&repo.web_url(), &stored.record);
            let Ok(repo) = parse_repository(&url, &servers) else {
                continue;
            };
            if !repos.iter().any(|known| known.host == repo.host) {
                repos.push(repo);
            }
        }

        repos.truncate(MAX_REPOSITORIES);
        repos
    }

    /// Saves the project label, then renames each selected copy and records each success.
    pub async fn rename_project(
        &self,
        project: &str,
        name: &str,
        repositories: &[RepositoryRename],
    ) -> Result<Vec<RepositoryRenameResult>> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 200 || name.chars().any(char::is_control) {
            return Err(FleetError::refused(
                "Use a project name of 1 to 200 characters without control characters.",
            ));
        }
        if repositories.len() > MAX_REPOSITORIES {
            return Err(FleetError::refused("Select at most 16 repository copies."));
        }

        let _rename = RENAMES.lock().await;
        let stored = find_project(&self.mesh, project).ok_or(FleetError::NotFound)?;
        let available = if repositories.is_empty() {
            Vec::new()
        } else {
            self.rename_targets(&stored).await
        };
        let servers = forgejo_of(&self.opened());
        let mut plans: Vec<RenamePlan> = Vec::new();

        for request in repositories {
            let url = current_url(&request.url, &stored.record);
            let repo = parse_repository(&url, &servers)?;
            if !available.contains(&repo) || plans.iter().any(|plan| plan.repo == repo) {
                return Err(FleetError::refused(
                    "Select each repository from this project's repository list once.",
                ));
            }
            let renamed = RepoName::from_str(&format!("{}/{}", repo.org(), request.name.trim()))
                .map_err(|error| FleetError::refused(error.to_string()))?;
            plans.push(RenamePlan {
                asked: request.url.clone(),
                repo,
                name: renamed,
            });
        }

        {
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
            stored.record.display_name = (name != stored.name).then(|| name.to_owned());
            write(&self.mesh, project_key(&stored.name), Some(&stored.record)).await?;
        }

        let mut results = Vec::new();
        for plan in plans {
            let result = match self.rename_copy(project, &plan).await {
                Ok(to) => RepositoryRenameResult::Renamed {
                    from: plan.asked,
                    to,
                },
                Err(error) => RepositoryRenameResult::Failed {
                    url: plan.asked,
                    message: error.to_string(),
                },
            };
            results.push(result);
        }
        Ok(results)
    }

    async fn rename_copy(&self, project: &str, plan: &RenamePlan) -> Result<String> {
        if plan.repo.name() == plan.name.repo() {
            return Ok(plan.repo.web_url());
        }
        let credentials = self.opened();
        let renamed = rename_repository(
            &self.hosts,
            &credentials,
            &plan.repo,
            &plan.name,
            self.github_api.as_deref(),
        )
        .await?;
        let from = plan.repo.web_url();
        let to = renamed.web_url();
        let saved = async {
            let _write = WRITES.lock().await;
            let mut stored = find_project(&self.mesh, project).ok_or(FleetError::NotFound)?;
            if stored.record.url == from {
                stored.record.url.clone_from(&to);
            }
            let change = RemoteRename {
                from,
                to: to.clone(),
            };
            if !stored.record.remote_renames.contains(&change) {
                stored.record.remote_renames.push(change);
            }
            write(&self.mesh, project_key(&stored.name), Some(&stored.record)).await
        }
        .await;
        saved.map_err(|error| {
            FleetError::refused(format!(
                "The repository was renamed to {to}, but saving the project failed: {error}"
            ))
        })?;
        Ok(to)
    }
}

fn current_url(url: &str, record: &ProjectRecord) -> String {
    let mut url = url.to_owned();
    for rename in &record.remote_renames {
        if url == rename.from {
            url.clone_from(&rename.to);
        }
    }
    url
}

/// Saved web URLs identify their Forgejo base even when its token is absent.
fn parse_repository(url: &str, servers: &[ForgejoUrl]) -> Result<RepoRef, ImportError> {
    RepoRef::parse(url, servers).or_else(|original| {
        let mut base = Url::parse(url).map_err(|_| original)?;
        base.path_segments_mut()
            .map_err(|_| ImportError::Malformed)?
            .pop_if_empty()
            .pop()
            .pop();
        let server = ForgejoUrl::from_str(base.as_str()).map_err(|_| ImportError::Malformed)?;
        RepoRef::parse(url, &[server])
    })
}
