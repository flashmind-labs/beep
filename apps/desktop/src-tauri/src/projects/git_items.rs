//! The user's open pull requests and issues on their git hosts, for Home.

use crate::{
    beep::epoch_ms,
    error::Result,
    fleet::{
        Fleet,
        events::Events,
        projects::{FleetState, desk},
        view_signal::View as ViewUpdate,
    },
    store::{
        cache_view,
        view_cache::{View, ViewCache},
    },
};
use beep_fleet::GitKept;
use beep_projects::git_items::board::OpenedItems;
use beep_proto::{GitHost, GitItem, LabeledGitItems};
use beep_proto::{node::events::Topic, watched};
use serde::Serialize;
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};
use tauri::{AppHandle, Manager, State};
use tokio::{sync::watch, time::MissedTickBehavior};

/// What Home lists.
#[derive(Debug, Clone, Serialize)]
pub struct GitItemsView {
    items: Vec<GitItemRow>,
    hosts: Vec<GitHostRow>,
}

#[derive(Debug, Clone, Serialize)]
struct GitHostRow {
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
    labels: Vec<String>,
}

impl From<GitHost> for GitHostRow {
    fn from(host: GitHost) -> Self {
        Self { error: host.error }
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

/// The open PRs and issues on every host the user has a token for, starting from what the last
/// run kept when the tokens and projects are the same.
#[tauri::command]
pub async fn git_items(
    fleet: FleetState<'_>,
    cache: State<'_, Arc<ViewCache>>,
) -> Result<Option<GitItemsView>> {
    let desk = desk(&fleet).await?;
    let kept = cache.get(View::GitItems).and_then(|saved| {
        serde_json::from_value::<GitKept>(saved)
            .map_err(|e| log::info!("the kept open items are unreadable: {e}"))
            .ok()
    });
    let done = desk.git_items_with(kept, None).await?;
    cache_view(&cache, View::GitItems, &done.kept);
    Ok(Some(done.items.into()))
}
/// Hosts without a push feed are read once for all windows. Results arrive as IPC events.
pub(crate) async fn forge_source(app: AppHandle, mut active: watch::Receiver<bool>) {
    loop {
        if watched::wait_for(&mut active, |active| *active)
            .await
            .is_err()
        {
            return;
        }

        let mut previous = None;
        let mut ticks = tokio::time::interval(Duration::from_secs(60));
        ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                changed = active.changed() => {
                    if changed.is_err() { return; }
                    if !watched::read_and_update(&mut active, |active| *active) { break; }
                },
                _ = ticks.tick() => {
                    let read = git_items(
                        app.state::<Arc<beep_fleet::Session>>(),
                        app.state::<Arc<ViewCache>>());
                    let result = tokio::select! {
                        result = read => result,
                        _ = watched::wait_for(&mut active, |active| !*active) => break,
                    };

                    match result {
                        Ok(Some(items)) => {
                            let current = serde_json::to_value(&items).ok();

                            if current != previous {
                                previous = current;
                                let fleet = app.state::<Arc<Fleet>>();
                                app.state::<Events>().publish_views(fleet.me, Topic::new("forge"),
                                    serde_json::json!({ "views": [{ "command": "git_items", "args": {}, "value": items }] }));
                            }
                        },
                        Ok(None) => {},
                        Err(error) => {
                            previous = None;
                            let fleet = app.state::<Arc<Fleet>>();
                            let view = ViewUpdate::failed("git_items", serde_json::json!({}), &error.to_string());
                            app.state::<Events>().publish_views(fleet.me, Topic::new("forge"), serde_json::json!({ "views": [view] }));
                        },
                    }
                }
            }
        }
    }
}
