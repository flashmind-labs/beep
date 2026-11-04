//! The user's open pull requests and issues on their git hosts, for Home.

use crate::{
    beep::{Service, epoch_ms},
    error::Result,
    projects::known,
};
use beep_proto::{GitHost, GitHostKind, GitItem, LabeledGitItems, ListLabeledGitItems};
use serde::Serialize;

/// What Home lists.
#[derive(Debug, Serialize)]
pub struct GitItemsView {
    items: Vec<GitItemRow>,
    hosts: Vec<GitHostRow>,
}

#[derive(Debug, Serialize)]
struct GitHostRow {
    host: String,
    forgejo: bool,
    account: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct GitItemRow {
    host: String,
    repo: String,
    number: u64,
    /// `pull_request` or `issue`.
    kind: &'static str,
    title: String,
    url: String,
    /// In milliseconds since the Unix epoch.
    updated_at: u64,
    authored: bool,
    assigned: bool,
    review_requested: bool,
    draft: bool,
    /// `none`, `approved` or `changes_requested`.
    review: &'static str,
    checks_failing: bool,
    comments: u32,
    labels: Vec<String>,
}

impl From<GitHost> for GitHostRow {
    fn from(host: GitHost) -> Self {
        Self {
            host: host.host,
            forgejo: matches!(host.kind, GitHostKind::Forgejo {}),
            account: host.account,
            error: host.error,
        }
    }
}

impl GitItemRow {
    fn labeled(item: GitItem, labels: Vec<String>) -> Self {
        Self {
            host: item.host,
            repo: item.repo,
            number: item.number,
            kind: item.kind.into(),
            title: item.title,
            url: item.url,
            updated_at: epoch_ms(item.updated_at),
            authored: item.authored,
            assigned: item.assigned,
            review_requested: item.review_requested,
            draft: item.draft,
            review: item.review.into(),
            checks_failing: item.checks_failing,
            comments: item.comments,
            labels,
        }
    }
}

impl From<LabeledGitItems> for GitItemsView {
    fn from(LabeledGitItems { found, labels }: LabeledGitItems) -> Self {
        Self {
            items: found
                .items
                .into_iter()
                .zip(labels)
                .map(|(item, labels)| GitItemRow::labeled(item, labels))
                .collect(),
            hosts: found.hosts.into_iter().map(GitHostRow::from).collect(),
        }
    }
}

/// The open PRs and issues; `None` from a server too old to list them.
#[tauri::command]
pub async fn git_items(beep: Service<'_>) -> Result<Option<GitItemsView>> {
    Ok(known(beep.call(ListLabeledGitItems {}).await)?.map(Into::into))
}
