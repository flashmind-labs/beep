//! The user's open pull requests and issues on their git hosts, for Home.

use super::forge_notices::{Found, Marked};
use crate::{
    beep::epoch_ms,
    error::Result,
    fleet::{
        Fleet,
        outbox::WindowOutbox,
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
use tokio::{
    sync::{mpsc, watch},
    time::MissedTickBehavior,
};

/// What Home lists.
#[derive(Debug, Clone, Serialize)]
pub struct GitItemsView {
    items: Vec<GitItemRow>,
    hosts: Vec<GitHostRow>,
}

#[derive(Debug, Clone, Serialize)]
struct GitHostRow {
    host: String,
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
        Self {
            host: host.host,
            error: host.error,
        }
    }
}

impl GitItemRow {
    fn marked(&self) -> Marked {
        Marked {
            host: self.host.clone(),
            repo: self.repo.clone(),
            number: self.number,
            title: self.title.clone(),
            url: self.url.clone(),
            updated_at: self.updated_at,
            review_requested: self.review_requested,
            assigned: self.assigned,
        }
    }

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

impl GitItemsView {
    /// The items as the notice diff reads them, and the hosts that answered.
    fn found(&self) -> Found {
        let answered = self.hosts.iter().filter(|host| host.error.is_none());
        Found {
            items: self.items.iter().map(GitItemRow::marked).collect(),
            answered: answered.map(|host| host.host.clone()).collect(),
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
/// Reads the git hosts every 60 s for as long as the app runs, for Home and the bell. Windows
/// that follow the `forge` topic get the views as IPC events. Each read goes to `reads`,
/// for the bell's notes. A phone reads only while a window follows.
pub(crate) async fn forge_source(
    app: AppHandle,
    mut active: watch::Receiver<bool>,
    reads: mpsc::UnboundedSender<Found>,
) {
    let mut previous = None;
    let mut ticks = tokio::time::interval(Duration::from_secs(60));
    ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        // A phone reads only while a window follows, to spare its battery.
        #[cfg(mobile)]
        if watched::wait_for(&mut active, |active| *active)
            .await
            .is_err()
        {
            return;
        }

        tokio::select! {
            changed = active.changed() => {
                if changed.is_err() { return; }
                // A window that starts following gets the current view at once.
                if !watched::read_and_update(&mut active, |active| *active) { continue; }
                previous = None;
            },
            _ = ticks.tick() => {}
        }

        let read = git_items(
            app.state::<Arc<beep_fleet::Session>>(),
            app.state::<Arc<ViewCache>>(),
        );
        let result = tokio::select! {
            result = read => result,
            changed = active.changed() => {
                if changed.is_err() { return; }
                watched::read_and_update(&mut active, |active| *active);
                continue;
            },
        };
        let following = watched::read_and_update(&mut active, |active| *active);

        match result {
            Ok(Some(items)) => {
                if reads.send(items.found()).is_err() {
                    return;
                }

                let current = serde_json::to_value(&items).ok();

                if following && current != previous {
                    previous = current;
                    let fleet = app.state::<Arc<Fleet>>();
                    app.state::<WindowOutbox>().publish_views(fleet.me, Topic::new("forge"),
                        serde_json::json!({ "views": [{ "command": "git_items", "args": {}, "value": items }] }));
                }
            }
            Ok(None) => {}
            Err(error) => {
                previous = None;

                if following {
                    let fleet = app.state::<Arc<Fleet>>();
                    let view =
                        ViewUpdate::failed("git_items", serde_json::json!({}), &error.to_string());
                    app.state::<WindowOutbox>().publish_views(
                        fleet.me,
                        Topic::new("forge"),
                        serde_json::json!({ "views": [view] }),
                    );
                }
            }
        }
    }
}
