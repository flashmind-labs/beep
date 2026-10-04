//! The daily sync of each connected mailbox and Slack, which Jev scores and
//! the memory learner reads. Each user's hourly sweep (see `User::start`) syncs whatever is due, and the
//! apps screen asks for a sync so a new connection is read right away.

pub mod mail;
pub mod slack;

use crate::{app::App, decision::DecisionTool, store::Feed, user::User};
use futures::future::join_all;
use std::{sync::Arc, time::Duration};

/// How long a synced feed waits for its next sync.
const SYNC_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
pub(crate) const SYNC_SWEEP: Duration = Duration::from_secs(60 * 60);

/// Syncs `feeds` of `user` in the background, each only if due.
pub fn spawn_sync(app: &Arc<App>, user: &Arc<User>, feeds: Vec<Feed>) {
    let (app, user) = (app.clone(), user.clone());
    tokio::spawn(async move { sync_user(&app, &user, &feeds).await });
}

/// Syncs each of `feeds` that is due.
pub(crate) async fn sync_user(app: &App, user: &User, feeds: &[Feed]) {
    let Some(decision) = app.decision() else {
        log::debug!("feeds: Jev is not configured; skipping sync");
        return;
    };
    join_all(
        feeds
            .iter()
            .map(|&feed| sync_feed(app, decision, user, feed)),
    )
    .await;
}

async fn sync_feed(app: &App, decision: &DecisionTool, user: &User, feed: Feed) {
    let slug = feed.slug();
    let Some(_claim) = user.claim_feed(feed) else {
        log::debug!("feeds: {slug} already syncing; user={user}");
        return;
    };
    match user.feed_synced_recently(feed, SYNC_EVERY).await {
        Ok(true) => {
            log::debug!("feeds: {slug} synced recently; user={user}");
            return;
        }
        Ok(false) => {}
        Err(error) => {
            log::warn!("feeds: unable to read sync state: {error:#}");
            return;
        }
    }
    log::info!("feeds: syncing {slug}; user={user}");
    let outcome = match feed {
        Feed::Mail(mailbox) => mail::sync(app, decision, user, mailbox)
            .await
            .map(|outcome| outcome.to_string()),
        Feed::Slack => slack::sync(app, decision, user)
            .await
            .map(|outcome| outcome.to_string()),
    };
    match outcome {
        Ok(outcome) => {
            log::info!("feeds: synced {slug}; user={user} {outcome}");
            if let Err(error) = user.mark_feed_synced(feed).await {
                log::warn!("feeds: unable to save sync state: {error:#}");
            }
        }
        // Not marked, so the next sweep tries again; progress so far is kept.
        Err(error) => log::warn!("feeds: sync of {slug} stopped: {error:#}; user={user}"),
    }
}
