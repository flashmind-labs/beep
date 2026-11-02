//! The user's open pull requests and issues on their git hosts, for Home.

use crate::{
    beep::{Beep, epoch_ms},
    error::Result,
    projects::unknown_op,
};
use beep_proto::{GitHost, GitHostKind, GitItem, GitItems, ListGitItems};
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

type Service<'a> = State<'a, Arc<Beep>>;

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

impl From<GitItem> for GitItemRow {
    fn from(item: GitItem) -> Self {
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
        }
    }
}

impl From<GitItems> for GitItemsView {
    fn from(found: GitItems) -> Self {
        Self {
            items: found.items.into_iter().map(GitItemRow::from).collect(),
            hosts: found.hosts.into_iter().map(GitHostRow::from).collect(),
        }
    }
}

/// The open PRs and issues; `None` from a server too old to list them.
#[tauri::command]
pub async fn git_items(beep: Service<'_>) -> Result<Option<GitItemsView>> {
    match beep.call(ListGitItems {}).await {
        Ok(found) => Ok(Some(found.into())),
        Err(e) if unknown_op(&e) => Ok(None),
        Err(e) => Err(e),
    }
}
