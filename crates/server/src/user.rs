//! One configured user and everything the server keeps for them: their rows,
//! memories, MCP connections, apps, starters and feed syncs. Everything
//! reached through a `User` is scoped to that user, so callers never pass a
//! user id, and the user's background work is spawned here too.
//!
//! Per-user store methods take a `UserKey`, and only this module can make
//! one, so no code outside a `User` can read or change a user's rows.

use crate::{
    AsyncMutex,
    app::{App, UserNotice},
    apps::{
        CATALOG, Composio, TOOLS_TIMEOUT, catalog_app, connect_url, connected_slugs, internal,
        mcp::{KEEP_ALIVE, StoreProvider, check_redirect, display_name, lost_login, preset},
        mcp_app,
    },
    feeds::{self, SYNC_SWEEP},
    memories::{
        CURATE_EVERY, CURATE_SWEEP, CURATE_SYSTEM, CURATE_TIMEOUT, Change, DUPLICATE, Exchange,
        Hit, LEARN_RULES, LEARN_SYSTEM, LEARN_TIMEOUT, LearnKind, LearnOp, Learned, MAX_CURATED,
        READING_LEARN_TIMEOUT, RECALLABLE, Reading, Remembered, Step, clip, curate_prompt, hit,
        learn_prompt, parse_curate_ops, parse_learn_ops, plan_curation, reading_prompt, settle,
    },
    protocol::{
        ApiError, AppIntegration, AppKind, ConnectLink, Cron, Integration, Memory, MemorySource,
        Notice, PhoneNotification, PromptSuggestions, Run, Session,
    },
    starters::{Cached, FALLBACK_FRESH, FRESH, Generator, StarterContext, TIMEOUT, fallback},
    store::{
        CreateRun, Fact, Feed, FinishedTurn, Location, MEMORY_CATEGORIES, McpServerRow, MemoryRow,
        RunStatus, Store, StoredContext,
    },
};
use anyhow::{Result, bail};
use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use flashmind::{
    core::Conversation,
    tools::{
        composio::{ComposioClient, make_composio_tool_wrappers},
        mcp::{
            McpRegistry,
            tools::{McpExecuteTool, McpSearchTool},
        },
    },
    types::Tool,
};
use secrecy::ExposeSecret;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fmt,
    sync::{Arc, Mutex, PoisonError, Weak},
    time::{Duration, Instant},
};
use tokio::{
    sync::{OnceCell, broadcast},
    time::{MissedTickBehavior, interval},
};

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
    key: UserKey,
    store: Store,
    /// This user, for parts that must reach back without keeping it alive.
    pub(crate) me: Weak<User>,
    pub(crate) composio: Option<Arc<Composio>>,
    /// MCP servers the user connected, such as Fastmail, made on first use.
    pub(crate) mcp: OnceCell<McpRegistry>,
    pub(crate) notifications: broadcast::Sender<UserNotice>,
    /// Suggested prompts, kept for 12 hours.
    pub(crate) starters: AsyncMutex<Option<Cached>>,
    /// Feeds being synced, so the hourly sweep and a new connection never
    /// read the same feed at once.
    syncing: Mutex<HashSet<Feed>>,
}

/// What every user is built from.
pub(crate) struct Shared {
    pub store: Store,
    pub notifications: broadcast::Sender<UserNotice>,
    pub composio: Option<Arc<Composio>>,
}

impl User {
    /// Builds the user `id` from the parts every user shares.
    pub(crate) fn new(id: &str, shared: &Shared) -> Arc<Self> {
        Arc::new_cyclic(|me| Self {
            id: id.to_owned(),
            key: UserKey(id.to_owned()),
            store: shared.store.clone(),
            me: me.clone(),
            composio: shared.composio.clone(),
            mcp: OnceCell::new(),
            notifications: shared.notifications.clone(),
            starters: AsyncMutex::new(None),
            syncing: Mutex::new(HashSet::new()),
        })
    }

    /// A user without Composio, for tests.
    #[doc(hidden)]
    pub fn for_test(store: Store, id: &str) -> Arc<Self> {
        let (notifications, _) = broadcast::channel(16);
        Self::new(
            id,
            &Shared {
                store,
                notifications,
                composio: None,
            },
        )
    }

    /// The user's id, as configured in the server's auth.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The user's date in their timezone, UTC when unknown.
    pub async fn today(&self) -> NaiveDate {
        let now = Utc::now();
        match self.get_timezone().await.map(|tz| tz.parse::<Tz>()) {
            Ok(Ok(tz)) => now.with_timezone(&tz).date_naive(),
            _ => now.date_naive(),
        }
    }

    /// Starts the user's background work: the MCP keep-alive, the hourly
    /// feed sync and the memory curation.
    pub fn start(self: &Arc<Self>, app: &Arc<App>) {
        let user = self.clone();
        tokio::spawn(async move {
            let mut ticks = interval(KEEP_ALIVE);
            // The first tick is immediate; the servers connect on first use.
            ticks.tick().await;
            loop {
                ticks.tick().await;
                user.keep_mcp_alive().await;
            }
        });

        let (sync_app, user) = (app.clone(), self.clone());
        tokio::spawn(async move {
            let mut ticks = interval(SYNC_SWEEP);
            ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
            loop {
                ticks.tick().await;
                feeds::sync_user(&sync_app, &user, &user.feeds().await).await;
            }
        });

        let (app, user) = (app.clone(), self.clone());
        tokio::spawn(async move {
            let mut ticks = interval(CURATE_SWEEP);
            loop {
                ticks.tick().await;
                user.curate_if_due(app.generator(user.id())).await;
            }
        });
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

    /// Starts a run for `message` in `session`. A repeated `request` returns
    /// its existing run.
    pub async fn create_run(
        &self,
        session: &str,
        request: &str,
        message: &str,
    ) -> Result<CreateRun> {
        self.store
            .create_run(&self.key, session, request, message)
            .await
    }

    /// The run with `id`, if it is the user's.
    pub async fn run(&self, id: &str) -> Result<Option<Run>> {
        self.store.run(&self.key, id).await
    }

    /// The user's sessions, pinned first, then newest first.
    pub async fn sessions(&self) -> Result<Vec<Session>> {
        self.store.sessions(&self.key).await
    }

    /// Renames a session; `false` when the user has no such session.
    pub async fn rename_session(&self, session_id: &str, title: &str) -> Result<bool> {
        self.store
            .rename_session(&self.key, session_id, title)
            .await
    }

    /// Deletes a session with its runs; `false` when the user has no such session.
    pub async fn delete_session(&self, session_id: &str) -> Result<bool> {
        self.store.delete_session(&self.key, session_id).await
    }

    /// Deletes sessions with their runs and returns how many existed.
    pub async fn delete_sessions(&self, ids: Vec<String>) -> Result<u64> {
        self.store.delete_sessions(&self.key, ids).await
    }

    /// Pins or unpins a session; `false` when the user has no such session.
    pub async fn pin_session(&self, session_id: &str, pinned: bool) -> Result<bool> {
        self.store.pin_session(&self.key, session_id, pinned).await
    }

    /// The full transcript of a session, as the phone shows it.
    pub async fn conversation(&self, session: &str) -> Result<Option<Conversation>> {
        self.store.conversation(&self.key, session).await
    }

    /// What the model sees for a session: the compacted context, or the
    /// transcript when the session was never compacted.
    pub async fn context(&self, session: &str) -> Result<Option<StoredContext>> {
        self.store.context(&self.key, session).await
    }

    /// Ends a run with `status`, saving its turn when there is one.
    pub async fn finish(
        &self,
        run: &Run,
        turn: Option<FinishedTurn<'_>>,
        status: RunStatus,
    ) -> Result<()> {
        self.store.finish(&self.key, run, turn, status).await
    }

    /// Records tool calls that failed, since the conversation keeps only their output.
    pub async fn record_tool_failures(&self, session: &str, call_ids: Vec<String>) -> Result<()> {
        self.store
            .record_tool_failures(&self.key, session, call_ids)
            .await
    }

    /// Ids of the session's tool calls that failed.
    pub async fn tool_failures(&self, session: &str) -> Result<HashSet<String>> {
        self.store.tool_failures(&self.key, session).await
    }

    /// Records Jev's memory score for the turn whose user message was sent at
    /// `turn_at`, in milliseconds since the epoch.
    pub async fn record_memory_score(
        &self,
        session: &str,
        turn_at: i64,
        probability: f64,
    ) -> Result<()> {
        self.store
            .record_memory_score(&self.key, session, turn_at, probability)
            .await
    }

    /// The session's memory scores by the time of each turn's user message.
    pub async fn memory_scores(&self, session: &str) -> Result<HashMap<i64, f64>> {
        self.store.memory_scores(&self.key, session).await
    }

    /// Stores a new memory and returns its id.
    pub async fn add_memory(&self, fact: Fact<'_>) -> Result<String> {
        self.store.add_memory(&self.key, fact).await
    }

    /// Replaces a memory's text, and its days when given. A memory the user
    /// stated stays stated. Returns whether it exists.
    pub async fn update_memory(&self, id: &str, fact: Fact<'_>) -> Result<bool> {
        self.store.update_memory(&self.key, id, fact).await
    }

    /// Replaces only a memory's text, as the user does from the Memories
    /// screen; it then counts as stated by the user.
    pub async fn edit_memory(&self, id: &str, content: &str) -> Result<bool> {
        self.store.edit_memory(&self.key, id, content).await
    }

    /// Every memory of the user, most recently changed first.
    pub async fn memory_rows(&self) -> Result<Vec<MemoryRow>> {
        self.store.memory_rows(&self.key).await
    }

    /// Deletes a memory; `false` when the user has no memory with `id`.
    pub async fn forget(&self, id: &str) -> Result<bool> {
        self.store.forget(&self.key, id).await
    }

    /// Deletes every memory of a user and returns how many there were.
    pub async fn forget_all(&self) -> Result<u64> {
        self.store.forget_all(&self.key).await
    }

    /// Every memory for the Memories screen, most recently changed first.
    pub async fn list_memories(&self) -> Result<Vec<Memory>> {
        self.store.list_memories(&self.key).await
    }

    /// Records that the memories were just curated.
    pub async fn mark_curated(&self) -> Result<()> {
        self.store.mark_curated(&self.key).await
    }

    /// The user's scheduled jobs, newest first.
    pub async fn list_crons(&self) -> Result<Vec<Cron>> {
        self.store.list_crons(&self.key).await
    }

    /// Schedules a repeating job and returns its id.
    pub async fn create_cron(&self, label: &str, schedule: &str, prompt: &str) -> Result<String> {
        self.store
            .create_cron(&self.key, label, schedule, prompt)
            .await
    }

    /// Schedules a job, once when `run_once`, and returns its id.
    pub async fn create_cron_ex(
        &self,
        label: &str,
        schedule: &str,
        prompt: &str,
        run_once: bool,
    ) -> Result<String> {
        self.store
            .create_cron_ex(&self.key, label, schedule, prompt, run_once)
            .await
    }

    /// Changes a job; `false` when the user has no such job.
    pub async fn update_cron(
        &self,
        id: &str,
        enabled: Option<bool>,
        label: Option<&str>,
        schedule: Option<&str>,
    ) -> Result<bool> {
        self.store
            .update_cron(&self.key, id, enabled, label, schedule)
            .await
    }

    /// Flips whether a job is enabled and returns the new state, or `None` when the user has no such job.
    pub async fn toggle_cron(&self, id: &str) -> Result<Option<bool>> {
        self.store.toggle_cron(&self.key, id).await
    }

    /// Deletes a job; `false` when the user has no such job.
    pub async fn delete_cron(&self, id: &str) -> Result<bool> {
        self.store.delete_cron(&self.key, id).await
    }

    /// Saves a phone's push token, replacing the one it had.
    pub async fn register_device(&self, device_id: &str, fcm_token: &str) -> Result<()> {
        self.store
            .register_device(&self.key, device_id, fcm_token)
            .await
    }

    /// Removes a phone's push token; `false` when it was not registered.
    pub async fn unregister_device(&self, device_id: &str) -> Result<bool> {
        self.store.unregister_device(&self.key, device_id).await
    }

    /// Records that a job started `run_id`.
    pub async fn log_cron_start(&self, cron_id: &str, run_id: &str) -> Result<()> {
        self.store.log_cron_start(&self.key, cron_id, run_id).await
    }

    /// When a job last started, if it ever did.
    pub async fn cron_last_run(&self, cron_id: &str) -> Result<Option<String>> {
        self.store.cron_last_run(&self.key, cron_id).await
    }

    /// Push tokens of the user's phones.
    pub async fn device_tokens(&self) -> Result<Vec<String>> {
        self.store.device_tokens(&self.key).await
    }

    /// The user's MCP connections as integrations.
    pub async fn list_mcp_connections(&self) -> Result<Vec<Integration>> {
        self.store.list_mcp_connections(&self.key).await
    }

    /// One MCP connection's config, if it is the user's.
    pub async fn get_mcp_connection(&self, id: &str) -> Result<Option<Value>> {
        self.store.get_mcp_connection(&self.key, id).await
    }

    /// Saves a new MCP connection and returns its id.
    pub async fn create_mcp_connection(
        &self,
        provider: &str,
        label: &str,
        config: &str,
    ) -> Result<String> {
        self.store
            .create_mcp_connection(&self.key, provider, label, config)
            .await
    }

    /// Replaces an MCP connection's credentials and status; `false` when the user has no such connection.
    pub async fn update_mcp_credentials(
        &self,
        id: &str,
        credentials: &str,
        status: &str,
    ) -> Result<bool> {
        self.store
            .update_mcp_credentials(&self.key, id, credentials, status)
            .await
    }

    /// Deletes an MCP connection; `false` when the user has no such connection.
    pub async fn delete_mcp_connection(&self, id: &str) -> Result<bool> {
        self.store.delete_mcp_connection(&self.key, id).await
    }

    /// The user's server-managed MCP servers.
    pub async fn mcp_servers(&self) -> Result<Vec<McpServerRow>> {
        self.store.mcp_servers(&self.key).await
    }

    /// Whether `feed` finished a sync within `every`.
    pub async fn feed_synced_recently(&self, feed: Feed, every: Duration) -> Result<bool> {
        self.store
            .feed_synced_recently(&self.key, feed, every)
            .await
    }

    /// Records that `feed` was just synced.
    pub async fn mark_feed_synced(&self, feed: Feed) -> Result<()> {
        self.store.mark_feed_synced(&self.key, feed).await
    }

    /// When the newest processed item of `feed` arrived.
    pub async fn feed_cursor(&self, feed: Feed) -> Result<Option<DateTime<Utc>>> {
        self.store.feed_cursor(&self.key, feed).await
    }

    /// Ids of the items of `feed` already processed.
    pub async fn feed_seen(&self, feed: Feed) -> Result<HashSet<String>> {
        self.store.feed_seen(&self.key, feed).await
    }

    /// Records an item of `feed` as read, so the next sync skips it.
    pub async fn mark_feed_seen(
        &self,
        feed: Feed,
        id: &str,
        received: DateTime<Utc>,
    ) -> Result<()> {
        self.store
            .mark_feed_seen(&self.key, feed, id, received)
            .await
    }

    /// Forgets processed ids older than `before`, which no run reads again.
    pub async fn prune_feed_seen(&self, feed: Feed, before: DateTime<Utc>) -> Result<usize> {
        self.store.prune_feed_seen(&self.key, feed, before).await
    }

    /// Saves a server's config. Credentials are untouched: they change only
    /// through `save_mcp_credentials`, so a stale config cannot roll back a
    /// rotated refresh token.
    pub async fn save_mcp_config(&self, name: &str, config: Value) -> Result<()> {
        self.store.save_mcp_config(&self.key, name, config).await
    }

    /// Saves a server's OAuth credentials; `Value::Null` clears them.
    pub async fn save_mcp_credentials(&self, name: &str, credentials: Value) -> Result<()> {
        self.store
            .save_mcp_credentials(&self.key, name, credentials)
            .await
    }

    /// Deletes a saved MCP server; `false` when the user had none by that name.
    pub async fn delete_mcp_server(&self, name: &str) -> Result<bool> {
        self.store.delete_mcp_server(&self.key, name).await
    }

    /// Saves the phone's latest location fix.
    pub async fn update_location(&self, fix: &Location) -> Result<()> {
        self.store.update_location(&self.key, fix).await
    }

    /// The phone's latest location, if it sent one.
    pub async fn get_location(&self) -> Result<Option<Location>> {
        self.store.get_location(&self.key).await
    }

    /// The user's timezone name from their last location, `UTC` when unknown.
    pub async fn get_timezone(&self) -> Result<String> {
        self.store.get_timezone(&self.key).await
    }

    /// Stores notifications sent by the phone and returns how many were new.
    pub async fn save_notifications(&self, notifications: Vec<PhoneNotification>) -> Result<usize> {
        self.store
            .save_notifications(&self.key, notifications)
            .await
    }

    /// Stored notifications matching every filter given, newest first.
    pub async fn query_notifications(
        &self,
        app: Option<&str>,
        since_minutes: Option<i64>,
        keyword: Option<&str>,
        limit: Option<i64>,
    ) -> Result<Vec<Value>> {
        self.store
            .query_notifications(&self.key, app, since_minutes, keyword, limit)
            .await
    }

    /// Starts tracking a habit and returns its id.
    pub async fn create_habit(&self, name: &str, frequency: &str) -> Result<String> {
        self.store.create_habit(&self.key, name, frequency).await
    }

    /// The user's habits that are not archived.
    pub async fn list_habits(&self) -> Result<Vec<Value>> {
        self.store.list_habits(&self.key).await
    }

    /// Logs one entry for a habit.
    pub async fn log_habit(&self, habit_name: &str, value: f64, note: Option<&str>) -> Result<()> {
        self.store
            .log_habit(&self.key, habit_name, value, note)
            .await
    }

    /// A habit's daily totals over the last `days`.
    pub async fn habit_status(&self, habit_name: &str, days: i64) -> Result<Value> {
        self.store.habit_status(&self.key, habit_name, days).await
    }

    /// Stops tracking a habit; `false` when the user has no such habit.
    pub async fn archive_habit(&self, habit_name: &str) -> Result<bool> {
        self.store.archive_habit(&self.key, habit_name).await
    }

    /// Create default cron jobs for a user if they have none yet.
    pub async fn ensure_default_crons(&self) -> Result<()> {
        self.store.ensure_default_crons(&self.key).await
    }

    /// Count notifications for a user received in the last `minutes` minutes.
    pub async fn notification_count_since(&self, minutes: i64) -> Result<i64> {
        self.store
            .notification_count_since(&self.key, minutes)
            .await
    }

    /// Whether the memories changed since they were last curated, at most
    /// once every `every`.
    pub async fn curation_due(&self, every: Duration) -> Result<bool> {
        self.store.curation_due(&self.key, every).await
    }
}

/// One user's memories.
impl User {
    /// Memories that still hold, ranked against `query`. An empty query lists
    /// the most recently changed.
    pub async fn search_memories(
        &self,
        query: &str,
        category: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Hit>> {
        let query = query.trim();
        let today = Utc::now().date_naive();
        let holds = |row: &MemoryRow| !row.valid.ended(today);
        if query.is_empty() {
            return Ok(self
                .memory_rows()
                .await?
                .into_iter()
                .filter(holds)
                .filter(|row| category.is_none_or(|c| row.category == c))
                .take(limit)
                .map(hit)
                .collect());
        }
        // Ended memories still in the store are dropped after the search.
        let found = self
            .store
            .search_memories(&self.key, query, category, limit * 2)
            .await?;
        Ok(found
            .into_iter()
            .filter(|found| found.score >= RECALLABLE && holds(&found.row))
            .take(limit)
            .map(|found| hit(found.row))
            .collect())
    }

    /// Stores a fact. When it says the same as a memory, that one is replaced.
    pub async fn remember(&self, fact: Fact<'_>) -> Result<Remembered> {
        let closest = self
            .store
            .closest_memory(&self.key, fact.content, DUPLICATE)
            .await?;
        if let Some(row) = closest {
            // The new text replaces the old one whole, days included.
            let replacement = Fact {
                valid: Some(fact.valid.unwrap_or_default()),
                ..fact.clone()
            };
            if self.update_memory(&row.id, replacement).await? {
                return Ok(Remembered {
                    id: row.id,
                    replaced: true,
                });
            }
            // Deleted meanwhile: add it instead.
        }
        let id = self.add_memory(fact).await?;
        Ok(Remembered {
            id,
            replaced: false,
        })
    }

    /// Reads a finished exchange and updates memory with what it teaches about
    /// the user, returning the changes made. Failures are logged and dropped.
    /// The memories closest to the latest exchange. Jev and the learner both
    /// see them, so a fact already known neither scores high nor is stored again.
    pub async fn related_memories(&self, exchange: &Exchange) -> Result<Vec<Hit>> {
        let query = format!(
            "{}\n{}",
            exchange.latest.message,
            clip(&exchange.latest.reply, 300)
        );
        self.search_memories(&query, None, 15).await
    }

    /// Updates memory with what the exchange teaches. `known` comes from
    /// `related_memories`.
    pub async fn learn(
        &self,
        generator: Generator<'_>,
        exchange: Exchange,
        known: &[Hit],
    ) -> Vec<Learned> {
        let attempt = async {
            if exchange.latest.message.split_whitespace().count() < 3 {
                return Ok(Vec::new());
            }
            let prompt = learn_prompt(known, &exchange);
            let answer = generator.text(LEARN_SYSTEM, prompt).await?;
            let ops = parse_learn_ops(&answer)?;
            let mut learned = Vec::new();
            for op in ops {
                learned.extend(
                    self.apply_learned(known, op, MemorySource::Learned {})
                        .await?,
                );
            }
            anyhow::Ok(learned)
        };
        settle(
            self.id(),
            tokio::time::timeout(LEARN_TIMEOUT, attempt).await,
        )
    }

    /// Reads an email or a Slack conversation and updates memory with what
    /// it teaches about the user, the same way `learn` does for a chat.
    pub async fn learn_reading(&self, generator: Generator<'_>, reading: &Reading) -> Vec<Learned> {
        let started = Instant::now();
        let attempt = async {
            let mut known = self.search_memories(&reading.query, None, 15).await?;
            // Anything can mention the user's name or family, which a search
            // on its text would not bring up, so personal facts always come along.
            for hit in self.search_memories("", Some("personal"), 15).await? {
                if !known.iter().any(|k| k.id == hit.id) {
                    known.push(hit);
                }
            }
            let system = format!("{}\n{LEARN_RULES}", reading.kind.intro());
            let answer = generator
                .text(&system, reading_prompt(&known, reading))
                .await?;
            let mut learned = Vec::new();
            for op in parse_learn_ops(&answer)? {
                learned.extend(
                    self.apply_learned(&known, op, reading.kind.source())
                        .await?,
                );
            }
            anyhow::Ok(learned)
        };
        let learned = settle(
            self.id(),
            tokio::time::timeout(READING_LEARN_TIMEOUT, attempt).await,
        );
        log::debug!(
            "memory {} learning took {:.1}s; id={}",
            reading.kind.source().as_str(),
            started.elapsed().as_secs_f32(),
            reading.id
        );
        learned
    }

    async fn apply_learned(
        &self,
        known: &[Hit],
        op: LearnOp,
        source: MemorySource,
    ) -> Result<Option<Learned>> {
        let shown = op
            .id
            .as_deref()
            .and_then(|id| known.iter().find(|h| h.id == id));
        if op.op == LearnKind::Forget {
            // Only what the extractor was shown can be forgotten.
            let Some(hit) = shown else {
                return Ok(None);
            };
            let forgotten = self.forget(&hit.id).await?;
            return Ok(forgotten.then(|| Learned {
                change: Change::Forgotten,
                id: hit.id.clone(),
                content: hit.content.clone(),
            }));
        }
        let Some(content) = op
            .content
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
        else {
            return Ok(None);
        };
        let fact = Fact {
            content,
            category: op
                .category
                .as_deref()
                .filter(|c| MEMORY_CATEGORIES.contains(c)),
            source,
            valid: op.validity(),
        };
        // An update of a memory it was not shown is a new fact.
        if let Some(hit) = shown
            && self.update_memory(&hit.id, fact.clone()).await?
        {
            return Ok(Some(Learned {
                change: Change::Updated,
                id: hit.id.clone(),
                content: content.to_owned(),
            }));
        }
        let fact = Fact {
            valid: Some(fact.valid.unwrap_or_default()),
            ..fact
        };
        let remembered = self.remember(fact).await?;
        Ok(Some(Learned {
            change: if remembered.replaced {
                Change::Updated
            } else {
                Change::Stored
            },
            id: remembered.id,
            content: content.to_owned(),
        }))
    }

    /// Merges duplicates, settles contradictions and drops stale memories of
    /// one user, returning how many memories changed.
    pub async fn curate_memories(&self, generator: Generator<'_>) -> Result<usize> {
        let rows: Vec<MemoryRow> = self
            .memory_rows()
            .await?
            .into_iter()
            .take(MAX_CURATED)
            .collect();
        if rows.len() < 2 {
            return Ok(0);
        }
        let today = Utc::now().date_naive();
        let attempt = async {
            let answer = generator
                .text(CURATE_SYSTEM, curate_prompt(&rows, today))
                .await?;
            parse_curate_ops(&answer)
        };
        let ops = tokio::time::timeout(CURATE_TIMEOUT, attempt)
            .await
            .map_err(|_| anyhow::anyhow!("curation timed out"))??;
        let by_id: HashMap<&str, &MemoryRow> = rows.iter().map(|r| (r.id.as_str(), r)).collect();
        let plan = plan_curation(ops, &by_id, today);
        let mut changed = 0;
        for step in plan {
            match step {
                Step::Forget(id) => {
                    changed += usize::from(self.forget(&id).await?);
                }
                Step::Rewrite { keep, fact, drop } => {
                    let fact = Fact {
                        content: &fact.content,
                        category: fact.category.as_deref(),
                        source: fact.source,
                        valid: fact.valid,
                    };
                    if !self.update_memory(&keep, fact).await? {
                        continue;
                    }
                    changed += 1;
                    for id in drop {
                        changed += usize::from(self.forget(&id).await?);
                    }
                }
            }
        }
        Ok(changed)
    }

    /// Curates the memories if they changed since the last pass, at most
    /// once a day.
    pub(crate) async fn curate_if_due(&self, generator: Generator<'_>) {
        match self.curation_due(CURATE_EVERY).await {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                log::warn!("unable to check memory curation: {error}; user={self}");
                return;
            }
        }
        match self.curate_memories(generator).await {
            Ok(0) => {}
            Ok(changed) => log::info!("memory curation changed {changed}; user={self}"),
            Err(error) => log::info!("memory curation skipped: {error}; user={self}"),
        }
        // Marked even after a failure, so a bad answer is retried tomorrow
        // rather than every hour.
        if let Err(error) = self.mark_curated().await {
            log::warn!("unable to mark memories curated: {error}; user={self}");
        }
    }
}

/// The user's MCP servers. The registry is built on first use.
impl User {
    /// Forgets a server whose login was rejected and tells the user to
    /// connect it again. The app then lists it as not connected.
    async fn drop_lost_mcp_login(&self, name: &str) {
        match self.remove_mcp(name).await {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                log::warn!("mcp: unable to forget a lost login; server={name} error={error:#}");
                return;
            }
        }
        let app = display_name(name);
        log::warn!("mcp: {name} rejected the saved login, asking to reconnect; user={self}");
        let notice = UserNotice {
            user: self.id().to_owned(),
            notice: Notice {
                title: format!("Reconnect {app}"),
                body: format!("Beep lost access to {app}. Tap to connect it again."),
                session_id: String::new(),
                app: Some(name.to_owned()),
            },
        };
        // Nobody listening just means the phone is offline; Apps still shows it.
        let _ = self.notifications.send(notice);
    }

    async fn mcp_registry(&self) -> &McpRegistry {
        self.mcp
            .get_or_init(|| async {
                let provider = Arc::new(StoreProvider {
                    user: self.me.clone(),
                });
                let registry = McpRegistry::new(provider, None);
                registry.load_saved().await;
                registry
            })
            .await
    }

    /// Names of the servers the user has credentials for.
    pub async fn mcp_connected(&self) -> Result<HashSet<String>> {
        Ok(self
            .mcp_servers()
            .await?
            .into_iter()
            .filter(|row| row.credentials.is_some())
            .map(|row| row.name)
            .collect())
    }

    /// Agent tools for the servers the user has connected: `mcp_search` to
    /// find a tool and load its schema, and `mcp` to call it. Two small tools
    /// instead of one per server tool keeps every prompt thousands of tokens
    /// shorter.
    pub async fn mcp_tools(&self) -> Vec<Arc<dyn Tool>> {
        match self.mcp_connected().await {
            Ok(connected) if !connected.is_empty() => {}
            Ok(_) => return Vec::new(),
            Err(error) => {
                log::warn!("mcp: unable to list servers: {error:#}");
                return Vec::new();
            }
        }
        let registry = self.mcp_registry().await;
        // The meta tools read `current_mcp_tools` on every call, so the queued
        // register and unregister ops are not needed.
        registry.drain_pending_ops();
        vec![
            Arc::new(McpSearchTool::new(registry.clone())),
            Arc::new(McpExecuteTool::new(registry.clone())),
        ]
    }

    /// Calls one tool on a connected server outside of any run and returns its
    /// result, parsed as JSON when the server answered with JSON text.
    pub async fn call_mcp(&self, server: &str, tool: &str, arguments: Value) -> Result<Value> {
        let registry = self.mcp_registry().await;
        let result = registry.call_tool(server, tool, arguments).await;
        registry.drain_pending_ops();
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                if lost_login(&error) {
                    self.drop_lost_mcp_login(server).await;
                }
                return Err(error);
            }
        };
        let text: String = result
            .content
            .iter()
            .filter_map(|content| content.as_text())
            .collect();
        if result.is_error {
            // Error text says what went wrong; it is clipped in case it echoes content.
            let reason: String = text.chars().take(200).collect();
            bail!("{server} {tool} returned an error: {reason}");
        }
        Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }

    /// Starts OAuth and returns the URL to open in the browser.
    pub async fn start_mcp_auth(&self, name: &str, redirect_uri: &str) -> Result<String> {
        let Some(config) = preset(name) else {
            bail!("unknown MCP server");
        };
        check_redirect(redirect_uri)?;
        let registry = self.mcp_registry().await;
        registry.add(config).await?;
        registry.start_auth(name, redirect_uri).await
    }

    /// Exchanges the code from the redirect, saves the tokens and connects.
    pub async fn finish_mcp_auth(
        &self,
        name: &str,
        code: &str,
        state: &str,
        issuer: Option<&str>,
    ) -> Result<()> {
        let registry = self.mcp_registry().await;
        registry.complete_auth(name, code, state, issuer).await
    }

    /// Disconnects and forgets the server and its tokens. Returns whether it existed.
    pub async fn remove_mcp(&self, name: &str) -> Result<bool> {
        let existed = self.mcp_connected().await?.contains(name);
        let registry = self.mcp_registry().await;
        if registry.get_config(name).await.is_some() {
            registry.remove(name).await?;
        } else {
            self.delete_mcp_server(name).await?;
        }
        registry.drain_pending_ops();
        Ok(existed)
    }

    /// Reconnects every connected server once. Connecting refreshes an
    /// expired access token, which also rotates and saves the refresh token.
    pub(crate) async fn keep_mcp_alive(&self) {
        let connected = match self.mcp_connected().await {
            Ok(connected) if !connected.is_empty() => connected,
            Ok(_) => return,
            Err(error) => {
                log::warn!("mcp: keep-alive unable to list servers: {error:#}");
                return;
            }
        };
        let registry = self.mcp_registry().await;
        for name in connected {
            if let Err(error) = registry.reconnect(&name).await {
                log::warn!("mcp: keep-alive reconnect failed; server={name} error={error:#}");
                if lost_login(&error) {
                    self.drop_lost_mcp_login(&name).await;
                }
            }
        }
        registry.drain_pending_ops();
    }
}

/// The app catalog as one user sees it: their Composio accounts and their
/// MCP servers.
impl User {
    /// A Composio client acting for the user, if the server has a key.
    pub fn composio_client(&self) -> Option<Arc<ComposioClient>> {
        let key = self.composio.as_ref()?.settings.key.expose_secret();
        Some(Arc::new(
            ComposioClient::new(key.to_owned(), None, None).with_entity_id(self.id().to_owned()),
        ))
    }

    /// Agent tools for the Composio apps `user` has connected. Only the
    /// connected toolkits are listed: the unfiltered catalog is thousands of
    /// tools across many pages. A slow or failing Composio gives no tools
    /// rather than holding up the run.
    pub async fn app_tools(&self) -> Vec<Arc<dyn Tool>> {
        let Some(client) = self.composio_client() else {
            return Vec::new();
        };
        let fetch = async {
            let toolkits: Vec<String> = connected_slugs(&client, self.id())
                .await?
                .into_iter()
                .filter(|slug| {
                    catalog_app(slug).is_some_and(|app| app.kind == AppKind::Composio {})
                })
                .collect();
            if toolkits.is_empty() {
                return Ok(Vec::new());
            }
            client.list_tools_tagged(&toolkits, &["important"]).await
        };
        match tokio::time::timeout(TOOLS_TIMEOUT, fetch).await {
            Ok(Ok(defs)) => make_composio_tool_wrappers(&client, &defs),
            Ok(Err(error)) => {
                log::warn!("apps: unable to list Composio tools: {error:#}");
                Vec::new()
            }
            Err(_) => {
                log::warn!("apps: listing Composio tools timed out");
                Vec::new()
            }
        }
    }

    /// The synced feeds `user` has connected, from Composio and MCP. An
    /// outage of either leaves its feeds out.
    pub async fn feeds(&self) -> Vec<Feed> {
        let mut connected = HashSet::new();
        if let Some(client) = self.composio_client() {
            match connected_slugs(&client, self.id()).await {
                Ok(slugs) => connected.extend(slugs),
                Err(error) => log::warn!("apps: unable to list connected accounts: {error:#}"),
            }
        }
        match self.mcp_connected().await {
            Ok(servers) => connected.extend(servers),
            Err(error) => log::warn!("apps: unable to list MCP servers: {error:#}"),
        }
        Feed::ALL
            .into_iter()
            .filter(|feed| connected.contains(feed.slug()))
            .collect()
    }

    /// The app catalog with what the user connected.
    pub async fn apps(&self) -> Result<Vec<AppIntegration>, ApiError> {
        // A Composio outage or bad key marks its apps unavailable instead of
        // failing the list, so apps that do not use Composio still show.
        let (connected, logos) = match (&self.composio, self.composio_client()) {
            (Some(composio), Some(client)) => {
                let (connected, logos) =
                    tokio::join!(connected_slugs(&client, self.id()), composio.logos());
                let connected = connected
                    .map_err(|error| {
                        log::warn!("apps: unable to list connected accounts: {error:#}")
                    })
                    .ok();
                (connected, logos)
            }
            _ => (None, HashMap::new()),
        };
        let composio = connected.is_some();
        let connected = connected.unwrap_or_default();
        let connected_mcp = self
            .mcp_connected()
            .await
            .map_err(internal("list MCP servers"))?;
        Ok(CATALOG
            .iter()
            .map(|app| {
                let (configured, connected) = match app.kind {
                    AppKind::Composio {} => (composio, connected.contains(app.slug)),
                    AppKind::Fastmail {} => (true, connected_mcp.contains(app.slug)),
                };
                AppIntegration {
                    slug: app.slug.to_owned(),
                    name: app.name.to_owned(),
                    description: app.description.to_owned(),
                    logo_url: logos.get(app.slug).cloned(),
                    connected,
                    configured,
                    kind: app.kind,
                }
            })
            .collect())
    }

    /// A link that connects a Composio app to the user.
    pub async fn connect_app(&self, slug: &str) -> Result<ConnectLink, ApiError> {
        let client = self.composio_app(slug)?;
        let callback = self
            .composio
            .as_ref()
            .and_then(|composio| composio.settings.callback_url.as_deref());
        let url = connect_url(&client, self.id(), slug, callback)
            .await
            .map_err(internal("start OAuth"))?;
        Ok(ConnectLink { url })
    }

    /// Starts OAuth for an MCP app. The browser comes back to `redirect_uri`
    /// on the phone, which passes the result to `finish_app_auth`.
    pub async fn start_app_auth(
        &self,
        slug: &str,
        redirect_uri: &str,
    ) -> Result<ConnectLink, ApiError> {
        mcp_app(slug)?;
        check_redirect(redirect_uri).map_err(|error| ApiError::bad_request(error.to_string()))?;
        let url = self
            .start_mcp_auth(slug, redirect_uri)
            .await
            .map_err(internal("start MCP OAuth"))?;
        Ok(ConnectLink { url })
    }

    /// Completes an MCP app login with the code the phone caught.
    pub async fn finish_app_auth(
        &self,
        slug: &str,
        code: &str,
        state: &str,
        issuer: Option<&str>,
    ) -> Result<(), ApiError> {
        mcp_app(slug)?;
        self.finish_mcp_auth(slug, code, state, issuer)
            .await
            .map_err(internal("finish MCP OAuth"))
    }

    /// Disconnects an app from the user.
    pub async fn disconnect_app(&self, slug: &str) -> Result<(), ApiError> {
        if catalog_app(slug).is_some_and(|app| app.kind == (AppKind::Fastmail {})) {
            let removed = self
                .remove_mcp(slug)
                .await
                .map_err(internal("remove MCP server"))?;
            return if removed {
                Ok(())
            } else {
                Err(ApiError::NotFound {})
            };
        }
        let user = self.id();
        let client = self.composio_app(slug)?;
        let accounts = client
            .list_connected_accounts_for_user(Some(slug), Some(user))
            .await
            .map_err(internal("list connected accounts"))?;
        let ids: Vec<String> = accounts
            .into_iter()
            .filter(|account| account.user_id.as_deref().is_none_or(|id| id == user))
            .filter(|account| account.toolkit_slug().is_none_or(|s| s == slug))
            .map(|account| account.id)
            .collect();
        if ids.is_empty() {
            return Err(ApiError::NotFound {});
        }
        for id in ids {
            client
                .delete_connected_account(&id)
                .await
                .map_err(internal("delete connected account"))?;
        }
        Ok(())
    }

    /// The client for a Composio app the user may connect or disconnect.
    fn composio_app(&self, slug: &str) -> Result<Arc<ComposioClient>, ApiError> {
        let app = catalog_app(slug).ok_or_else(|| ApiError::bad_request("unknown app"))?;
        if app.kind == (AppKind::Fastmail {}) {
            return Err(ApiError::bad_request(
                "Fastmail connects through StartAppAuth with a localhost redirect.",
            ));
        }
        self.composio_client().ok_or_else(|| {
            ApiError::bad_request("App connections are not configured on this server.")
        })
    }
}

/// A user's starters are kept for 12 hours.
impl User {
    /// Starters made in the last 12 hours, if any.
    pub async fn cached_starters(&self) -> Option<PromptSuggestions> {
        let cache = self.starters.lock().await;
        let entry = cache.as_ref().filter(|c| c.expires > Instant::now())?;
        Some(entry.suggestions.clone())
    }

    /// Asks the model, falling back on any failure, and caches the result.
    pub async fn generate_starters(
        &self,
        generator: Generator<'_>,
        context: impl Future<Output = StarterContext>,
    ) -> PromptSuggestions {
        let attempt = async {
            let context = context.await;
            generator.ask(&context).await
        };
        let (suggestions, personal) = match tokio::time::timeout(TIMEOUT, attempt).await {
            Ok(Ok(suggestions)) => (suggestions, true),
            Ok(Err(error)) => {
                log::info!("prompt suggestions fell back: {error}");
                (fallback(), false)
            }
            Err(_) => {
                log::info!("prompt suggestions fell back: timed out");
                (fallback(), false)
            }
        };
        let fresh = if personal { FRESH } else { FALLBACK_FRESH };
        let suggestions = PromptSuggestions {
            suggestions,
            personal,
        };
        *self.starters.lock().await = Some(Cached {
            expires: Instant::now() + fresh,
            suggestions: suggestions.clone(),
        });
        suggestions
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
    use crate::feeds::mail::Mailbox;
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

    fn fact(content: &str) -> Fact<'_> {
        Fact {
            content,
            category: None,
            source: MemorySource::User {},
            valid: None,
        }
    }

    #[tokio::test]
    async fn memories_dedupe_within_a_user_and_stay_apart_across_users() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = User::for_test(store.clone(), "alice");
        let bob = User::for_test(store, "bob");

        let first = alice.remember(fact("I live in Lisbon")).await.unwrap();
        assert!(!first.replaced);
        let again = alice.remember(fact("I live in Lisbon")).await.unwrap();
        assert!(again.replaced);
        assert_eq!(again.id, first.id);

        // The same words from another user are a new memory, not a replacement.
        let theirs = bob.remember(fact("I live in Lisbon")).await.unwrap();
        assert!(!theirs.replaced);
        assert_ne!(theirs.id, first.id);

        let found = alice.search_memories("Lisbon", None, 10).await.unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, first.id);
        assert!(!bob.forget(&first.id).await.unwrap());
        assert_eq!(alice.list_memories().await.unwrap().len(), 1);
    }
}
