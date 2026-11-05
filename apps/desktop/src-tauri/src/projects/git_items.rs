//! The user's open pull requests and issues on their git hosts, for Home.

use crate::{
    beep::epoch_ms,
    center::{
        cache_view,
        view_cache::{View, ViewCache},
    },
    error::Result,
    fleet::projects::{FleetState, desk},
};
use beep_fleet::GitKept;
use beep_projects::git_items::board::OpenedItems;
use beep_proto::{GitHost, GitHostKind, GitItem, LabeledGitItems};
use serde::Serialize;
use std::{sync::Arc, time::SystemTime};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc;

/// The page hears the list each time a source changes it, under this name.
const PROGRESS: &str = "git-items";
/// Lists waiting to be sent to the page.
const PENDING: usize = 4;

/// What Home lists.
#[derive(Debug, Clone, Serialize)]
pub struct GitItemsView {
    items: Vec<GitItemRow>,
    hosts: Vec<GitHostRow>,
}

#[derive(Debug, Clone, Serialize)]
struct GitHostRow {
    host: String,
    forgejo: bool,
    account: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
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
    /// When the host says it was opened, in milliseconds since the Unix epoch.
    opened_at: Option<u64>,
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
    fn labeled(item: GitItem, labels: Vec<String>, opened_at: Option<SystemTime>) -> Self {
        Self {
            host: item.host,
            repo: item.repo,
            number: item.number,
            kind: item.kind.into(),
            title: item.title,
            url: item.url,
            updated_at: epoch_ms(item.updated_at),
            opened_at: opened_at.map(epoch_ms),
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

impl From<OpenedItems> for GitItemsView {
    fn from(OpenedItems { items, opened_at }: OpenedItems) -> Self {
        let LabeledGitItems { found, labels } = items;
        Self {
            items: found
                .items
                .into_iter()
                .zip(labels)
                .zip(opened_at)
                .map(|((item, labels), opened)| GitItemRow::labeled(item, labels, opened))
                .collect(),
            hosts: found.hosts.into_iter().map(GitHostRow::from).collect(),
        }
    }
}

/// The open PRs and issues on every host the user has a token for. What the last run kept is sent
/// to the page first when the tokens and projects are the same, then the list again each time a
/// token's search or a project's lists arrive, as `git-items` events; the reply is the final list.
#[tauri::command]
pub async fn git_items(
    app: AppHandle,
    fleet: FleetState<'_>,
    cache: State<'_, Arc<ViewCache>>,
) -> Result<Option<GitItemsView>> {
    let desk = desk(&fleet).await?;
    let kept = cache.get(View::GitItems).and_then(|saved| {
        serde_json::from_value::<GitKept>(saved)
            .map_err(|e| log::info!("the kept open items are unreadable: {e}"))
            .ok()
    });
    let (tx, mut rx) = mpsc::channel(PENDING);
    let run = async move { desk.git_items_with(kept, Some(&tx)).await };
    let show = async {
        while let Some(items) = rx.recv().await {
            if let Err(e) = app.emit(PROGRESS, GitItemsView::from(items)) {
                log::debug!("Home missed an open items update: {e}");
            }
        }
    };
    let (done, ()) = tokio::join!(run, show);
    let done = done?;
    cache_view(&cache, View::GitItems, &done.kept);
    Ok(Some(done.items.into()))
}
