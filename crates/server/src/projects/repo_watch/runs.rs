//! Where a project's workflow runs are read: which of the user's tokens reads its host.

use crate::{
    account::{
        credentials::{Opened, Slot, github_for, is_public},
        user::User,
    },
    app::App,
    projects::repo_watch::hosts::RepoOnHost,
};
use beep_projects::{git_items::HostError, model::IssueHost, repo_watch::runs::Target};

/// Where `place`'s runs are read for `user_id` with credentials `opened`, or why they cannot be.
pub(crate) async fn target(
    user_id: &str,
    opened: &[Opened],
    place: &RepoOnHost,
) -> Result<Target, String> {
    match &place.host {
        IssueHost::Github => {
            let opened = github_for(opened, place.repo.owner()).ok_or_else(|| {
                "Add a GitHub token in Settings to see this project's runs.".to_owned()
            })?;
            let account = opened
                .github
                .as_ref()
                .map(|login| login.login.as_str())
                .unwrap_or_default();
            Ok(Target::github(
                user_id,
                place.repo.clone(),
                opened.secret.clone(),
                account,
            ))
        }
        IssueHost::Forgejo(name) => {
            let (opened, url) = opened
                .iter()
                .find_map(|opened| match &opened.slot {
                    Slot::Forgejo(url) if url.hostname().eq_ignore_ascii_case(name.as_str()) => {
                        Some((opened, url))
                    }
                    _ => None,
                })
                .ok_or_else(|| {
                    format!(
                        "Add a token for {} in Settings to see this project's runs.",
                        name.as_str()
                    )
                })?;
            if !is_public(url).await {
                return Err(HostError::Private(url.host()).to_string());
            }
            Ok(Target::forgejo(
                user_id,
                place.repo.clone(),
                url,
                opened.secret.clone(),
            ))
        }
    }
}

/// The user's git credentials, opened, or why they did not open.
pub(crate) async fn credentials(app: &App, user: &User) -> Result<Vec<Opened>, String> {
    user.git_credentials(&app.credential_key)
        .await
        .map_err(|e| {
            log::warn!("runs: the git credentials did not open: {e:#}");
            "Beep could not open your git tokens.".to_owned()
        })
}
