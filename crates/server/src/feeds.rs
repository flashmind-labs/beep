//! The daily sync of each connected mailbox and Slack, which Jev scores and
//! the memory learner reads. An hourly sweep syncs whatever is due, and the
//! apps screen asks for a sync so a new connection is read right away.

use crate::{app::App, decision::DecisionTool, mail, slack, store::Feed};
use futures::future::join_all;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

/// How long a synced feed waits for its next sync.
const SYNC_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
const SYNC_SWEEP: Duration = Duration::from_secs(60 * 60);

/// A user's feed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FeedKey {
    user: String,
    feed: Feed,
}

/// The feeds being synced.
#[derive(Default)]
pub struct Syncing(Mutex<HashSet<FeedKey>>);

impl Syncing {
    /// Claims `feed` of `user` for one sync, or `None` while another sync has it.
    fn claim(&self, user: &str, feed: Feed) -> Option<FeedClaim<'_>> {
        let key = FeedKey {
            user: user.to_owned(),
            feed,
        };
        let inserted = self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.clone());
        // Built only once the lock is released: dropping a claim takes the lock.
        inserted.then(|| FeedClaim { syncing: self, key })
    }
}

/// Holds a feed for one sync; dropping it lets the next sync in.
struct FeedClaim<'a> {
    syncing: &'a Syncing,
    key: FeedKey,
}

impl Drop for FeedClaim<'_> {
    fn drop(&mut self) {
        self.syncing
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.key);
    }
}

/// Syncs `feeds` of `user` in the background, each only if due.
pub fn spawn_sync(app: &Arc<App>, user: &str, feeds: Vec<Feed>) {
    let (app, user) = (app.clone(), user.to_owned());
    tokio::spawn(async move { sync_user(&app, &user, &feeds).await });
}

/// Runs the sync once at start and then every hour.
pub fn start(app: Arc<App>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(SYNC_SWEEP);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            for user in app.auth.user_ids() {
                let feeds = app.apps.feeds(user).await;
                sync_user(&app, user, &feeds).await;
            }
        }
    });
}

async fn sync_user(app: &App, user: &str, feeds: &[Feed]) {
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

async fn sync_feed(app: &App, decision: &DecisionTool, user: &str, feed: Feed) {
    let slug = feed.slug();
    let Some(_claim) = app.syncing.claim(user, feed) else {
        log::debug!("feeds: {slug} already syncing; user={user}");
        return;
    };
    match app.store.feed_synced_recently(user, feed, SYNC_EVERY).await {
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
            if let Err(error) = app.store.mark_feed_synced(user, feed).await {
                log::warn!("feeds: unable to save sync state: {error:#}");
            }
        }
        // Not marked, so the next sweep tries again; progress so far is kept.
        Err(error) => log::warn!("feeds: sync of {slug} stopped: {error:#}; user={user}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_claimed_feed_cannot_be_claimed_again_until_released() {
        let syncing = Syncing::default();
        let claim = syncing.claim("alice", Feed::Slack).expect("feed is free");
        assert!(syncing.claim("alice", Feed::Slack).is_none());
        assert!(syncing.claim("bob", Feed::Slack).is_some());
        assert!(syncing.claim("alice", Feed::Slack).is_none());
        drop(claim);
        assert!(syncing.claim("alice", Feed::Slack).is_some());
    }
}
