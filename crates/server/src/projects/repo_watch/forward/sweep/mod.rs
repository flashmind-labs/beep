//! Webhooks left on repositories the user has no project for any more: an online node removes
//! them over a watch that only removes. It runs when a project is removed and when a node
//! connects, so rows kept while every node was offline go then.

use super::session::{new_secret, watch_host};
use crate::{
    account::user::User,
    app::App,
    nodes::Online,
    protocol::{
        SecretText,
        node::webhook::{Watch, WatchReply},
    },
    store::OrphanWebhooks,
};
use anyhow::{Context, Result};
use beep_projects::model::{IssueHost, RepoName};
use secrecy::ExposeSecret;
use std::time::Duration;
use tokio::time::timeout;

/// The longest one node may take over one repository's removals.
const LIMIT: Duration = Duration::from_secs(30);

/// Has the first online node of `user` that can remove them do so.
pub(crate) async fn sweep_any(app: &App, user: &User) {
    let nodes = app.nodes.of_user(user.id());
    for node in nodes.iter().filter(|node| node.forwards_webhooks()) {
        match sweep(user, node).await {
            Ok(true) => return,
            Ok(false) => {}
            Err(e) => log::warn!("webhooks: old webhooks not looked up: {e:#}"),
        }
    }
}

/// Has `node` remove the user's orphaned webhooks; whether there were none.
pub(crate) async fn sweep(user: &User, node: &Online) -> Result<bool> {
    let orphans = user.orphan_webhooks().await?;
    for orphan in &orphans {
        match timeout(LIMIT, remove(user, node, orphan)).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => log::info!("webhooks: old webhooks not removed: {e:#}"),
            Err(_) => log::info!("webhooks: removing old webhooks took too long"),
        }
    }
    Ok(orphans.is_empty())
}

async fn remove(user: &User, node: &Online, orphan: &OrphanWebhooks) -> Result<()> {
    let host = IssueHost::from_stored(&orphan.host).context("an unknown host")?;
    let repo = RepoName::parse(&orphan.repo)?;
    let watch = Watch {
        host: watch_host(&host, &repo),
        repo: orphan.repo.clone(),
        secret: SecretText::new(new_secret().expose_secret()),
        stale: orphan.hooks.clone(),
        remove_only: true,
    };
    // Held to the end: a node from before makes a webhook as well, and
    // removes it once this closes.
    let (_tx, mut rx) = node.watch_webhook(&watch).await?;
    while let Some(reply) = rx.recv::<WatchReply>().await? {
        match reply {
            WatchReply::Removed { hook } => {
                user.webhook_removed(&orphan.host, &orphan.repo, hook)
                    .await?;
            }
            WatchReply::Live { .. } => break,
            WatchReply::Polling { why, detail } => {
                log::info!("webhooks: old webhooks not removed ({why}): {detail}");
                break;
            }
            other => log::debug!("webhooks: skipped {other:?}"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
