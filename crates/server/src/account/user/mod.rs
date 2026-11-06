//! One user and everything the server keeps for them, scoped so callers never pass a user id.
//! Only this module makes a `UserKey`, so no code outside a `User` can reach a user's rows.

mod apps;
mod credentials;
mod events;
mod feed_sync;
mod groups;
mod keys;
mod learned;
mod machines;
mod mcp;
mod mcp_config;
mod memories;
mod places;
mod project_work;
mod projects;
mod routines;
mod runs;
mod sessions;
mod starters;
mod usage;

use crate::{
    AsyncMutex,
    agent::decision::DecisionTool,
    app::{App, UserNotice},
    apps::feeds::{self, SYNC_SWEEP},
    apps::{
        Composio,
        mcp::{KEEP_ALIVE, McpSlot},
    },
    memory::memories::CURATE_SWEEP,
    memory::starters::Cached,
    store::{Account, Feed, Store},
};
use chrono::{NaiveDate, Utc};
use chrono_tz::Tz;
use flashmind::{core::CancellationToken, tools::mcp::McpRegistry};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt,
    sync::{Arc, Mutex, PoisonError, Weak},
    time::Instant,
};
use tokio::{
    sync::{OnceCell, broadcast},
    time::{Interval, MissedTickBehavior, interval},
};

pub use apps::AppTools;
pub use events::Topics;

/// Proof that a store call is made for one user. Only `User` makes one.
#[derive(Clone)]
pub struct UserKey(String);

impl UserKey {
    /// The user id the key was made for.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// A key for `id`, so store tests can call per-user methods.
    #[cfg(test)]
    pub(crate) fn for_test(id: &str) -> Self {
        Self(id.to_owned())
    }
}

pub struct User {
    id: String,
    /// The id Composio and the sandbox know the user by.
    external_id: String,
    key: UserKey,
    store: Store,
    /// This user, for parts that must reach back without keeping it alive.
    pub(crate) me: Weak<User>,
    pub(crate) composio: Option<Arc<Composio>>,
    /// Files memories under a category; without it they stay unfiled.
    decision: Option<Arc<DecisionTool>>,
    /// MCP servers the user connected, such as Fastmail, made on first use.
    pub(crate) mcp: Arc<McpSlot>,
    /// The servers the user added by URL, which connect only when called.
    pub(crate) custom_mcp: OnceCell<McpRegistry>,
    /// Sign-ins to added servers under way, by their OAuth `state`.
    pub(crate) mcp_flows: Mutex<HashMap<String, mcp::Flow>>,
    pub(crate) notifications: broadcast::Sender<UserNotice>,
    /// Suggested prompts, kept for 12 hours.
    pub(crate) starters: AsyncMutex<Option<Cached>>,
    /// When the user last asked for new starters, oldest first.
    starter_refreshes: Mutex<VecDeque<Instant>>,
    /// Feeds being synced, so two syncs never read the same feed at once.
    syncing: Mutex<HashSet<Feed>>,
    /// Ends the background loops `start` began, once the account is gone.
    stopped: CancellationToken,
}

/// What every user is built from.
pub(crate) struct Shared {
    pub store: Store,
    pub notifications: broadcast::Sender<UserNotice>,
    pub composio: Option<Arc<Composio>>,
    pub decision: Option<Arc<DecisionTool>>,
}

impl User {
    /// Builds the user `id` from the parts every user shares.
    pub(crate) fn new(account: &Account, shared: &Shared) -> Arc<Self> {
        Arc::new_cyclic(|me| Self {
            id: account.id.clone(),
            external_id: account.external_id.clone(),
            key: UserKey(account.id.clone()),
            store: shared.store.clone(),
            me: me.clone(),
            composio: shared.composio.clone(),
            decision: shared.decision.clone(),
            mcp: Arc::default(),
            custom_mcp: OnceCell::new(),
            mcp_flows: Mutex::default(),
            notifications: shared.notifications.clone(),
            starters: AsyncMutex::new(None),
            starter_refreshes: Mutex::default(),
            syncing: Mutex::new(HashSet::new()),
            stopped: CancellationToken::new(),
        })
    }

    /// A user without Composio, for tests.
    #[doc(hidden)]
    pub fn for_test(store: Store, id: &str) -> Arc<Self> {
        let (notifications, _) = broadcast::channel(16);
        Self::new(
            &Account::new(id),
            &Shared {
                store,
                notifications,
                composio: None,
                decision: None,
            },
        )
    }

    /// The user's id, as configured in the server's auth.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The id outside services, such as Composio, know the user by.
    pub fn external_id(&self) -> &str {
        &self.external_id
    }

    /// The user's date in their timezone, UTC when unknown.
    pub async fn today(&self) -> NaiveDate {
        let now = Utc::now();
        match self.timezone().await {
            Some(tz) => now.with_timezone(&tz).date_naive(),
            None => now.date_naive(),
        }
    }

    /// The user's saved timezone; `None` when it does not read.
    pub(crate) async fn timezone(&self) -> Option<Tz> {
        match self.get_timezone().await {
            Ok(name) => name.parse().ok(),
            Err(e) => {
                log::warn!("timezone not read; user={} error={e:#}", self.id());
                None
            }
        }
    }

    /// Starts the MCP keep-alive, the hourly feed sync and memory curation.
    pub fn start(self: &Arc<Self>, app: &Arc<App>) {
        let user = self.clone();
        tokio::spawn(async move {
            let mut ticks = interval(KEEP_ALIVE);
            // The first tick is immediate; the servers connect on first use.
            ticks.tick().await;
            while user.tick(&mut ticks).await {
                user.keep_mcp_alive().await;
            }
        });

        let (sync_app, user) = (app.clone(), self.clone());
        tokio::spawn(async move {
            let mut ticks = interval(SYNC_SWEEP);
            ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
            while user.tick(&mut ticks).await {
                feeds::sync_user(&sync_app, &user, &user.feeds().await).await;
            }
        });

        let (app, user) = (app.clone(), self.clone());
        tokio::spawn(async move {
            let mut ticks = interval(CURATE_SWEEP);
            while user.tick(&mut ticks).await {
                match user.file_memories().await {
                    Ok(0) => {}
                    Ok(filed) => log::info!("filed {filed} memories; user={user}"),
                    Err(error) => log::warn!("unable to file memories: {error}; user={user}"),
                }
                user.curate_if_needed(app.generator(user.id())).await;
            }
        });
    }

    /// Waits for the next tick; false once the user is stopped.
    async fn tick(&self, ticks: &mut Interval) -> bool {
        tokio::select! {
            _ = self.stopped.cancelled() => false,
            _ = ticks.tick() => true,
        }
    }

    /// Ends the loops `start` began.
    pub fn stop(&self) {
        self.stopped.cancel();
    }

    /// Claims `feed` for one sync, or `None` while another sync has it.
    pub(crate) fn claim_feed(&self, feed: Feed) -> Option<FeedClaim<'_>> {
        let inserted = self
            .syncing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(feed);
        // Built lazily and only once the lock is released: dropping a claim
        // takes the lock and frees the feed.
        inserted.then(|| FeedClaim { user: self, feed })
    }
}

/// Holds a feed for one sync; dropping it lets the next sync in.
pub(crate) struct FeedClaim<'a> {
    user: &'a User,
    feed: Feed,
}

impl Drop for FeedClaim<'_> {
    fn drop(&mut self) {
        self.user
            .syncing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.feed);
    }
}

/// The id, for logs.
impl fmt::Display for User {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::feeds::mail::Mailbox;
    use crate::store::TestDb;

    #[tokio::test]
    async fn a_claimed_feed_cannot_be_claimed_again_until_released() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = User::for_test(store, "alice");
        let claim = user.claim_feed(Feed::Slack).expect("feed is free");
        assert!(user.claim_feed(Feed::Slack).is_none());
        assert!(user.claim_feed(Feed::Mail(Mailbox::Gmail)).is_some());
        assert!(user.claim_feed(Feed::Slack).is_none());
        drop(claim);
        assert!(user.claim_feed(Feed::Slack).is_some());
    }

    #[tokio::test]
    async fn actions_are_kept_newest_first_per_user() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = User::for_test(store.clone(), "alice");
        let bob = User::for_test(store, "bob");
        alice
            .record_actions(&["Gmail: send email".into()])
            .await
            .unwrap();
        alice
            .record_actions(&["Calendar: create event".into()])
            .await
            .unwrap();
        let done = alice.recent_actions(10).await.unwrap();
        let done: Vec<&str> = done.iter().map(|row| row.description.as_str()).collect();
        assert_eq!(done, ["Calendar: create event", "Gmail: send email"]);
        assert!(bob.recent_actions(10).await.unwrap().is_empty());
    }
}
