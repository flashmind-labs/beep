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
        mcp::{
            KEEP_ALIVE, StoreProvider, app_tools, check_redirect, display_name, lost_login, preset,
        },
        mcp_app,
    },
    decision::DecisionTool,
    feeds::{self, SYNC_SWEEP, mail::Mailbox},
    images::Picture,
    memories::{
        CURATE_EVERY, CURATE_SWEEP, CURATE_SYSTEM, CURATE_TIMEOUT, Change, DUPLICATE, Exchange,
        FILE_CONCURRENCY, FILE_QUESTION, FILING, Hit, LEARN_RULES, LEARN_SYSTEM, LEARN_TIMEOUT,
        LearnKind, LearnOp, Learned, MAX_CURATED, MAX_FILED, READING_LEARN_TIMEOUT, RECALLABLE,
        Reading, Remembered, Step, clip, core_ids, curate_prompt, hit, learn_prompt,
        parse_curation, parse_learn_ops, plan_curation, profile, reading_prompt, settle,
    },
    permissions::{self, Rules, ToolSlug},
    places::PlaceEvent,
    protocol::{
        ApiError, AppIntegration, AppKind, ConnectLink, Cron, FilePart, ImageRef, Memory,
        MemorySource, Notice, PromptSuggestion, PromptSuggestions, Run, SentImages, Session,
    },
    starters::{
        Cached, FALLBACK_FRESH, FRESH, Generator, STARTER_REFRESH_WINDOW, STARTER_REFRESHES,
        StarterContext, TIMEOUT, fallback,
    },
    store::{
        Account, AccountKey, ActionRow, AppPermissionRow, AttentionItem, CopiedShare, CreateRun,
        Crossed, DeletedTurn, Fact, Feed, FinishedTurn, FoundSession, Join, JoinedGroup, LinkKey,
        Location, MEMORY_CATEGORIES, McpServerRow, MemoryRow, NewFile, NewKey, NewMessage,
        NewPicture, NewPlace, NewPlaceRoutine, Origin, PicturesOf, PlaceRoutine, Posted,
        ProfileNote, ProfileRead, PushDevice, RunStatus, SavedPlace, Share, Store, StoredContext,
        TurnModel, TurnsToDelete,
    },
};
use anyhow::{Result, bail};
use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use flashmind::{
    core::Conversation,
    cron::CronJob,
    tools::{
        composio::{ComposioClient, make_composio_tool_wrappers},
        mcp::McpRegistry,
    },
    types::{Model, ReasoningLevel, Tool},
};
use futures::{StreamExt, stream};
use secrecy::ExposeSecret;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt,
    sync::{Arc, Mutex, PoisonError, Weak},
    time::{Duration, Instant},
};
use tokio::{
    sync::{OnceCell, broadcast},
    time::{MissedTickBehavior, interval},
};
use uuid::Uuid;

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
    pub(crate) mcp: OnceCell<McpRegistry>,
    pub(crate) notifications: broadcast::Sender<UserNotice>,
    /// Suggested prompts, kept for 12 hours.
    pub(crate) starters: AsyncMutex<Option<Cached>>,
    /// When the user last asked for new starters, oldest first.
    starter_refreshes: Mutex<VecDeque<Instant>>,
    /// Feeds being synced, so the hourly sweep and a new connection never
    /// read the same feed at once.
    syncing: Mutex<HashSet<Feed>>,
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
            mcp: OnceCell::new(),
            notifications: shared.notifications.clone(),
            starters: AsyncMutex::new(None),
            starter_refreshes: Mutex::default(),
            syncing: Mutex::new(HashSet::new()),
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

    /// The id outside services, such as Composio and the sandbox, know the
    /// user by.
    pub fn external_id(&self) -> &str {
        &self.external_id
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
                match user.file_memories().await {
                    Ok(0) => {}
                    Ok(filed) => log::info!("filed {filed} memories; user={user}"),
                    Err(error) => log::warn!("unable to file memories: {error}; user={user}"),
                }
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

    /// The run already started for `request`, without starting one.
    pub async fn existing_run(
        &self,
        session: &str,
        request: &str,
        message: &str,
    ) -> Result<Option<CreateRun>> {
        self.store
            .run_for_request(self.key.as_str(), session, request, message)
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

    /// Up to `limit` sessions whose transcript holds `query`, ignoring case.
    pub async fn search_sessions(&self, query: &str, limit: i64) -> Result<Vec<FoundSession>> {
        self.store.search_sessions(&self.key, query, limit).await
    }

    /// Renames a session; `false` when the user has no such session.
    pub async fn rename_session(&self, session_id: &str, title: &str) -> Result<bool> {
        let renamed = self
            .store
            .rename_session(&self.key, session_id, title)
            .await?;
        self.changed("");
        Ok(renamed)
    }

    /// Deletes a session with its runs; `false` when the user has no such session.
    pub async fn delete_session(&self, session_id: &str) -> Result<bool> {
        let deleted = self.store.delete_session(&self.key, session_id).await?;
        self.changed("");
        Ok(deleted)
    }

    /// Deletes sessions with their runs and returns how many existed.
    pub async fn delete_sessions(&self, ids: Vec<String>) -> Result<u64> {
        let deleted = self.store.delete_sessions(&self.key, ids).await?;
        self.changed("");
        Ok(deleted)
    }

    /// Removes one turn of a session, starting at the user message sent at
    /// `timestamp` in milliseconds since the epoch, or that turn and the
    /// ones after it.
    pub async fn delete_turn(
        &self,
        session_id: &str,
        timestamp: i64,
        which: TurnsToDelete,
    ) -> Result<DeletedTurn> {
        let deleted = self
            .store
            .delete_turn(&self.key, session_id, timestamp, which)
            .await?;
        if deleted == DeletedTurn::Deleted {
            self.changed(session_id);
        }
        Ok(deleted)
    }

    /// The ones of `ids` the user still has.
    pub async fn existing_sessions(&self, ids: Vec<String>) -> Result<Vec<String>> {
        self.store.existing_sessions(&self.key, ids).await
    }

    /// Pins or unpins a session; `false` when the user has no such session.
    pub async fn pin_session(&self, session_id: &str, pinned: bool) -> Result<bool> {
        let pinned = self
            .store
            .pin_session(&self.key, session_id, pinned)
            .await?;
        self.changed("");
        Ok(pinned)
    }

    /// Keeps a file Beep sends in `session_id` and returns its id.
    pub(crate) async fn save_file(
        &self,
        session_id: &str,
        name: &str,
        mime: &str,
        data: &[u8],
    ) -> Result<String> {
        let file = NewFile {
            session_id,
            name,
            mime,
            data,
        };
        self.store.save_file(&self.key, file).await
    }

    /// Keeps the pictures the user sent with a message.
    pub(crate) async fn save_pictures(
        &self,
        of: PicturesOf<'_>,
        pictures: &[Picture],
    ) -> Result<Vec<ImageRef>> {
        let pictures: Vec<NewPicture> = pictures
            .iter()
            .map(|picture| NewPicture {
                mime: &picture.mime,
                data: &picture.data,
                width: picture.width,
                height: picture.height,
            })
            .collect();
        self.store.save_pictures(&self.key, of, &pictures).await
    }

    /// The pictures the user sent in a session, by turn.
    pub(crate) async fn session_pictures(&self, session_id: &str) -> Result<Vec<SentImages>> {
        self.store.session_pictures(&self.key, session_id).await
    }

    /// Up to `len` bytes of a sent file or picture from `offset` on, or
    /// `None` when it is gone or not the user's to read.
    pub(crate) async fn file_part(
        &self,
        id: &str,
        offset: u64,
        len: u64,
    ) -> Result<Option<FilePart>> {
        self.store.file_part(&self.key, id, offset, len).await
    }

    /// Shares `session_id` as `conversation`; `None` when the user has no
    /// such session.
    pub async fn share_session(
        &self,
        session_id: &str,
        conversation: &Conversation,
    ) -> Result<Option<Share>> {
        self.store
            .share_session(&self.key, session_id, conversation)
            .await
    }

    /// Stops sharing `session_id`; `false` when it was not shared.
    pub async fn unshare_session(&self, session_id: &str) -> Result<bool> {
        self.store.unshare_session(&self.key, session_id).await
    }

    /// Copies the share with `token` into the user's chats as `session_id`.
    pub async fn copy_share(&self, token: &str, session_id: &str) -> Result<CopiedShare> {
        let copied = self.store.copy_share(&self.key, token, session_id).await?;
        if copied == CopiedShare::Copied {
            self.changed("");
        }
        Ok(copied)
    }

    /// Makes the group `id` with the user in it; `false` when the id is taken.
    pub async fn create_group(&self, id: &str, name: &str, member_name: &str) -> Result<bool> {
        self.store
            .create_group(&self.key, id, name, member_name)
            .await
    }

    /// The groups the user is in, most recently active first.
    pub async fn groups(&self) -> Result<Vec<JoinedGroup>> {
        self.store.groups(&self.key).await
    }

    /// The group `id`, if the user is in it.
    pub async fn group(&self, id: &str) -> Result<Option<JoinedGroup>> {
        self.store.group(&self.key, id).await
    }

    /// Posts to a group the user is in; `None` when they are not.
    pub async fn post_to_group(&self, message: NewMessage<'_>) -> Result<Option<Posted>> {
        self.store.post_to_group(&self.key, message).await
    }

    /// The invite token of a group the user is in, a new one with `reset`.
    pub async fn group_invite(&self, id: &str, reset: bool) -> Result<Option<String>> {
        self.store.group_invite(&self.key, id, reset).await
    }

    /// Joins the group behind `token`.
    pub async fn join_group(&self, token: &str, name: &str) -> Result<Join> {
        self.store.join_group(&self.key, token, name).await
    }

    /// Pins or unpins a group; `false` when the user is not in it.
    pub async fn pin_group(&self, id: &str, pinned: bool) -> Result<bool> {
        self.store.pin_group(&self.key, id, pinned).await
    }

    /// Lets Beep use the user's apps in a group when others ask, or stops
    /// it; `false` when the user is not in it.
    pub async fn share_group_apps(&self, id: &str, shared: bool) -> Result<bool> {
        self.store.share_group_apps(&self.key, id, shared).await
    }

    /// Leaves a group; `false` when the user was not in it.
    pub async fn leave_group(&self, id: &str) -> Result<bool> {
        self.store.leave_group(&self.key, id).await
    }

    /// Tells the user's phones that `session_id` changed, or the chat list
    /// when it is empty.
    pub fn changed(&self, session_id: &str) {
        // Nobody listening just means no phone is online; it syncs on connect.
        let _ = self
            .notifications
            .send(UserNotice::changed(self.id(), session_id));
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

    /// Ends a run with `status`, saving its turn when there is one. Returns
    /// the session's new `updated_at`, `None` when no turn was saved.
    pub async fn finish(
        &self,
        run: &Run,
        turn: Option<FinishedTurn<'_>>,
        status: RunStatus,
    ) -> Result<Option<DateTime<Utc>>> {
        let saved = turn.is_some();
        let updated_at = self.store.finish(&self.key, run, turn, status).await?;
        if saved {
            self.changed(&run.session_id);
        }
        Ok(updated_at)
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

    /// Records the model that answered the turn whose user message was sent
    /// at `turn_at`, in milliseconds since the epoch.
    pub async fn record_turn_model(
        &self,
        session: &str,
        turn_at: i64,
        model: &TurnModel,
    ) -> Result<()> {
        self.store
            .record_turn_model(&self.key, session, turn_at, model)
            .await
    }

    /// The session's models by the time of each turn's user message.
    pub async fn turn_models(&self, session: &str) -> Result<HashMap<i64, TurnModel>> {
        self.store.turn_models(&self.key, session).await
    }

    /// The session's memory scores by the time of each turn's user message.
    pub async fn memory_scores(&self, session: &str) -> Result<HashMap<i64, f64>> {
        self.store.memory_scores(&self.key, session).await
    }

    /// Stores a new memory, filed by Jev, and returns its id.
    pub async fn add_memory(&self, fact: Fact<'_>) -> Result<String> {
        let category = self.file(fact.content).await;
        let fact = Fact {
            category: category.or(fact.category),
            ..fact
        };
        self.store.add_memory(&self.key, fact).await
    }

    /// Replaces a memory's text, and its days when given, and files it again.
    /// A memory the user stated stays stated. Returns whether it exists.
    pub async fn update_memory(&self, id: &str, fact: Fact<'_>) -> Result<bool> {
        let category = self.file(fact.content).await;
        let fact = Fact {
            category: category.or(fact.category),
            ..fact
        };
        self.store.update_memory(&self.key, id, fact).await
    }

    /// Replaces only a memory's text, as the user does from the Memories
    /// screen, and files it again; it then counts as stated by the user.
    pub async fn edit_memory(&self, id: &str, content: &str) -> Result<bool> {
        let category = self.file(content).await;
        self.store
            .edit_memory(&self.key, id, content, category)
            .await
    }

    /// Jev's category for a memory, `None` when Jev is not set up or failed.
    async fn file(&self, content: &str) -> Option<&'static str> {
        let decision = self.decision.as_ref()?;
        match decision.choose(content, FILE_QUESTION, &FILING).await {
            Ok(category) => Some(category),
            Err(error) => {
                log::warn!("unable to file a memory: {error}; user={self}");
                None
            }
        }
    }

    /// Files the memories that are unfiled or under a category no longer
    /// used, a few at a time. Returns how many it filed.
    pub(crate) async fn file_memories(&self) -> Result<usize> {
        if self.decision.is_none() {
            return Ok(0);
        }
        let rows: Vec<MemoryRow> = self
            .memory_rows()
            .await?
            .into_iter()
            .filter(|row| !MEMORY_CATEGORIES.contains(&row.category.as_str()))
            .take(MAX_FILED)
            .collect();
        let filed: Vec<(String, &str)> = stream::iter(rows)
            .map(|row| async move { self.file(&row.content).await.map(|c| (row.id, c)) })
            .buffer_unordered(FILE_CONCURRENCY)
            .filter_map(|filed| async move { filed })
            .collect()
            .await;
        let mut changed = 0;
        for (id, category) in filed {
            changed += usize::from(self.store.set_category(&self.key, &id, category).await?);
        }
        Ok(changed)
    }

    /// Every memory of the user, most recently changed first.
    pub async fn memory_rows(&self) -> Result<Vec<MemoryRow>> {
        self.store.memory_rows(&self.key).await
    }

    /// The memory profile for a chat's context, `None` when it is empty.
    pub async fn memory_profile(&self) -> Result<Option<String>> {
        Ok(profile(&self.memory_rows().await?, self.today().await))
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

    /// One of the user's jobs as the scheduler sees it.
    pub async fn cron_job(&self, id: Uuid) -> Result<Option<CronJob>> {
        self.store.cron_job(id, Some(self.key.as_str())).await
    }

    /// Deletes a job; `false` when the user has no such job.
    pub async fn delete_cron(&self, id: &str) -> Result<bool> {
        self.store.delete_cron(&self.key, id).await
    }

    /// Saves a phone's push token, replacing the one it had.
    pub async fn register_device(&self, device: PushDevice<'_>) -> Result<()> {
        self.store.register_device(&self.key, device).await
    }

    /// Removes a phone's push token; `false` when it was not registered.
    pub async fn unregister_device(&self, device_id: &str) -> Result<bool> {
        self.store.unregister_device(&self.key, device_id).await
    }

    /// Links another device's key to this account.
    pub async fn link_key(&self, key: NewKey<'_>) -> Result<LinkKey> {
        self.store.link_key(&self.key, key).await
    }

    /// The account's primary key, then the keys linked to it.
    pub async fn account_keys(&self) -> Result<Vec<AccountKey>> {
        self.store.account_keys(&self.key).await
    }

    /// Stops a linked key signing in; `false` when the account has no such key.
    pub async fn revoke_key(&self, endpoint_id: &str) -> Result<bool> {
        self.store.revoke_key(&self.key, endpoint_id).await
    }

    /// Keeps what a turn did in the user's apps.
    pub async fn record_actions(&self, descriptions: &[String]) -> Result<()> {
        self.store.record_actions(&self.key, descriptions).await
    }

    /// The latest things Beep did in the user's apps, newest first.
    pub async fn recent_actions(&self, limit: i64) -> Result<Vec<ActionRow>> {
        self.store.recent_actions(&self.key, limit).await
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

    /// How far the profile pass got through `source`.
    pub(crate) async fn profile_read(&self, source: &str) -> Result<ProfileRead> {
        self.store.profile_read(&self.key, source).await
    }

    /// Saves what the profile pass noted and how far it read.
    pub(crate) async fn add_profile_notes(
        &self,
        source: &str,
        notes: &[ProfileNote],
        read_before: Option<DateTime<Utc>>,
    ) -> Result<()> {
        self.store
            .add_profile_notes(&self.key, source, notes, read_before)
            .await
    }

    pub(crate) async fn finish_profile_read(&self, source: &str) -> Result<()> {
        self.store.finish_profile_read(&self.key, source).await
    }

    pub(crate) async fn reset_profile_read(&self, source: &str) -> Result<()> {
        self.store.reset_profile_read(&self.key, source).await
    }

    pub(crate) async fn profile_notes(&self) -> Result<Vec<ProfileNote>> {
        self.store.profile_notes(&self.key).await
    }

    /// The input schema of `tool` on MCP server `server`, once it has connected.
    pub(crate) async fn mcp_tool_schema(&self, server: &str, tool: &str) -> Option<Value> {
        let tools = self.mcp_registry().await.current_mcp_tools();
        tools
            .get(server)?
            .iter()
            .find(|def| def.name == tool)
            .map(|def| def.input_schema.clone())
    }

    /// When `feed` last finished a sync.
    pub async fn feed_synced_at(&self, feed: Feed) -> Result<Option<DateTime<Utc>>> {
        self.store.feed_synced_at(&self.key, feed).await
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

    /// Makes the digest routine `job` unless one was made before.
    pub async fn offer_digest(&self, job: &CronJob) -> Result<bool> {
        self.store.offer_digest(&self.key, job).await
    }

    /// Whether the user has a digest routine switched on.
    pub async fn digest_enabled(&self) -> Result<bool> {
        self.store.digest_enabled(&self.key).await
    }

    /// Keeps an email for the evening digest.
    pub async fn flag_attention(&self, item: &AttentionItem) -> Result<()> {
        self.store.flag_attention(&self.key, item).await
    }

    /// Emails waiting for the digest, oldest first; older than `keep` are dropped.
    pub async fn attention_items(&self, keep: Duration) -> Result<Vec<AttentionItem>> {
        self.store.attention_items(&self.key, keep).await
    }

    /// Forgets flagged emails of `mailbox` the digest dealt with.
    pub async fn clear_attention(&self, mailbox: Mailbox, ids: &[String]) -> Result<()> {
        self.store.clear_attention(&self.key, mailbox, ids).await
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

    /// What Beep may do in each app.
    pub(crate) async fn app_rules(&self) -> Result<Rules> {
        Ok(Rules::from_rows(
            self.store.app_permissions(&self.key).await?,
        ))
    }

    /// The model the user picked, if any.
    pub(crate) async fn model(&self) -> Result<Option<String>> {
        self.store.user_model(&self.key).await
    }

    pub(crate) async fn set_model(&self, model: &Model) -> Result<()> {
        self.store
            .set_user_model(&self.key, Some(&model.to_string()))
            .await
    }

    /// The reasoning level the user picked, if any.
    pub(crate) async fn thinking(&self) -> Result<Option<ReasoningLevel>> {
        let picked = self.store.user_thinking(&self.key).await?;
        Ok(picked.and_then(|level| level.parse().ok()))
    }

    pub(crate) async fn set_thinking(&self, level: ReasoningLevel) -> Result<()> {
        self.store
            .set_user_thinking(&self.key, &level.to_string())
            .await
    }

    /// Whether Jev picks the model and thinking per prompt.
    pub(crate) async fn auto_model(&self) -> Result<bool> {
        self.store.user_auto_model(&self.key).await
    }

    pub(crate) async fn set_auto_model(&self, enabled: bool) -> Result<()> {
        self.store.set_user_auto_model(&self.key, enabled).await
    }

    pub(crate) async fn save_app_permissions(&self, row: &AppPermissionRow) -> Result<()> {
        self.store.save_app_permissions(&self.key, row).await
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

    /// The user's saved places, by name.
    pub async fn list_places(&self) -> Result<Vec<SavedPlace>> {
        self.store.list_places(&self.key).await
    }

    /// The place called `name`, ignoring case.
    pub async fn place_named(&self, name: &str) -> Result<Option<SavedPlace>> {
        self.store.place_named(&self.key, name).await
    }

    /// Saves a place, or moves the one with its name.
    pub async fn save_place(&self, place: &NewPlace<'_>) -> Result<SavedPlace> {
        self.store.save_place(&self.key, place).await
    }

    /// Renames a place or changes its radius; `None` when there is no such place.
    pub async fn update_place(
        &self,
        id: Uuid,
        name: Option<&str>,
        radius: Option<u32>,
    ) -> Result<Option<SavedPlace>> {
        self.store.update_place(&self.key, id, name, radius).await
    }

    /// Forgets a place and its routines; `false` when there is no such place.
    pub async fn delete_place(&self, id: Uuid) -> Result<bool> {
        self.store.delete_place(&self.key, id).await
    }

    /// Records which places `fix` is at, and returns the ones it arrived at
    /// or left.
    pub async fn cross_places(&self, fix: &Location) -> Result<Vec<Crossed>> {
        self.store.cross_places(&self.key, fix).await
    }

    /// Saves a routine that starts at a place and returns its id.
    pub async fn create_place_routine(&self, routine: &NewPlaceRoutine<'_>) -> Result<Uuid> {
        let id = self.store.create_place_routine(&self.key, routine).await?;
        self.changed("");
        Ok(id)
    }

    /// The enabled routines that start on `event` at `place`.
    pub async fn place_routines(
        &self,
        place: Uuid,
        event: PlaceEvent,
    ) -> Result<Vec<PlaceRoutine>> {
        self.store.place_routines(&self.key, place, event).await
    }

    /// Records that routine `id` runs now, unless it ran at or after `since`.
    pub async fn claim_place_run(
        &self,
        id: Uuid,
        now: DateTime<Utc>,
        since: Option<DateTime<Utc>>,
    ) -> Result<bool> {
        self.store.claim_place_run(&self.key, id, now, since).await
    }

    /// Saves the timezone the phone is set to.
    pub async fn set_timezone(&self, timezone: &str) -> Result<()> {
        self.store.set_timezone(&self.key, timezone).await
    }

    /// The user's timezone name, `UTC` when unknown.
    pub async fn get_timezone(&self) -> Result<String> {
        self.store.get_timezone(&self.key).await
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
                let origin = op.origin(&exchange.session_id);
                learned.extend(
                    self.apply_learned(known, op, MemorySource::Learned {}, origin)
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
            // on its text would not bring up, so those facts always come along.
            for category in ["identity", "people"] {
                for hit in self.search_memories("", Some(category), 15).await? {
                    if !known.iter().any(|k| k.id == hit.id) {
                        known.push(hit);
                    }
                }
            }
            let system = format!("{}\n{LEARN_RULES}", reading.kind.intro());
            let answer = generator
                .text(&system, reading_prompt(&known, reading))
                .await?;
            let mut learned = Vec::new();
            for op in parse_learn_ops(&answer)? {
                let origin = op.origin(&reading.id);
                learned.extend(
                    self.apply_learned(&known, op, reading.kind.source(), origin)
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
        origin: Origin,
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
            category: None,
            source,
            valid: op.validity(),
            origin: Some(origin),
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
            parse_curation(&answer)
        };
        let curation = tokio::time::timeout(CURATE_TIMEOUT, attempt)
            .await
            .map_err(|_| anyhow::anyhow!("curation timed out"))??;
        let by_id: HashMap<&str, &MemoryRow> = rows.iter().map(|r| (r.id.as_str(), r)).collect();
        let plan = plan_curation(curation.ops, &by_id, today);
        let core = curation.core.map(|core| core_ids(core, &plan, &by_id));
        let mut changed = 0;
        for step in plan {
            match step {
                Step::Forget(id) => {
                    changed += usize::from(self.forget(&id).await?);
                }
                Step::Rewrite { keep, fact, drop } => {
                    let fact = Fact {
                        content: &fact.content,
                        category: None,
                        source: fact.source,
                        valid: fact.valid,
                        origin: None,
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
        // Only the memories curation read move in or out of the profile.
        if let Some(core) = core {
            for row in rows.iter().filter(|row| row.core != core.contains(&row.id)) {
                changed += usize::from(self.store.set_core(&self.key, &row.id, !row.core).await?);
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
                approval: None,
                changed: false,
                group: false,
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

    /// Agent tools for the servers the user has connected, one per remote
    /// tool Beep exposes.
    pub async fn mcp_tools(&self) -> Vec<Arc<dyn Tool>> {
        let connected = match self.mcp_connected().await {
            Ok(connected) if !connected.is_empty() => connected,
            Ok(_) => return Vec::new(),
            Err(error) => {
                log::warn!("mcp: unable to list servers: {error:#}");
                return Vec::new();
            }
        };
        let registry = self.mcp_registry().await;
        // The tools are made from `current_mcp_tools` on every run, so the
        // queued register and unregister ops are not needed.
        registry.drain_pending_ops();
        app_tools(registry, &connected)
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

/// The Composio tools of a user's connected apps.
#[derive(Default)]
pub struct AppTools {
    pub tools: Vec<Arc<dyn Tool>>,
    /// Slugs whose Composio tags say they create or destroy something.
    pub(crate) hinted: HashSet<ToolSlug>,
}

/// The app catalog as one user sees it: their Composio accounts and their
/// MCP servers.
impl User {
    /// A Composio client acting for the user, if the server has a key.
    pub fn composio_client(&self) -> Option<Arc<ComposioClient>> {
        let key = self.composio.as_ref()?.settings.key.expose_secret();
        Some(Arc::new(
            ComposioClient::new(key.to_owned(), None, None)
                .with_entity_id(self.external_id().to_owned()),
        ))
    }

    /// Agent tools for the Composio apps `user` has connected. Only the
    /// connected toolkits are listed: the unfiltered catalog is thousands of
    /// tools across many pages. A slow or failing Composio gives no tools
    /// rather than holding up the run.
    pub async fn app_tools(&self) -> AppTools {
        let Some(client) = self.composio_client() else {
            return AppTools::default();
        };
        let Some(composio) = &self.composio else {
            return AppTools::default();
        };
        let fetch = async {
            let connected = connected_slugs(&client, self.external_id()).await?;
            let slugs: Vec<&str> = connected
                .iter()
                .filter_map(|slug| catalog_app(slug))
                .flat_map(|app| app.tools.iter().map(|tool| tool.slug))
                .collect();
            composio.tools(&slugs).await
        };
        match tokio::time::timeout(TOOLS_TIMEOUT, fetch).await {
            Ok(Ok(defs)) => AppTools {
                hinted: defs
                    .iter()
                    .filter(|def| permissions::hinted_to_act(&def.tags))
                    .map(|def| ToolSlug::new(&def.slug))
                    .collect(),
                tools: make_composio_tool_wrappers(&client, &defs),
            },
            Ok(Err(error)) => {
                log::warn!("apps: unable to list Composio tools: {error:#}");
                AppTools::default()
            }
            Err(_) => {
                log::warn!("apps: listing Composio tools timed out");
                AppTools::default()
            }
        }
    }

    /// The synced feeds `user` has connected, from Composio and MCP. An
    /// outage of either leaves its feeds out.
    pub async fn feeds(&self) -> Vec<Feed> {
        let mut connected = HashSet::new();
        if let Some(client) = self.composio_client() {
            match connected_slugs(&client, self.external_id()).await {
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
                let (connected, logos) = tokio::join!(
                    connected_slugs(&client, self.external_id()),
                    composio.logos()
                );
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
        let url = connect_url(&client, self.external_id(), slug, callback)
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
        let user = self.external_id();
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

/// A user's starters are kept for 12 hours, until one of them is about an
/// event that is over, or until the phone moves to another city.
impl User {
    /// Drops the cached starters, so the next ones see what just changed.
    pub async fn forget_starters(&self) {
        *self.starters.lock().await = None;
    }

    /// Drops the cached starters when `message` is one of them, so the next
    /// empty chat gets a new set instead of the starter just used.
    pub async fn forget_used_starter(&self, message: &str) {
        let mut cache = self.starters.lock().await;
        let used = cache.as_ref().is_some_and(|cached| {
            cached
                .suggestions
                .suggestions
                .iter()
                .any(|starter| starter.prompt.trim() == message.trim())
        });
        if used {
            *cache = None;
        }
    }

    /// Counts a request for new starters. False when the user already made
    /// `STARTER_REFRESHES` within `STARTER_REFRESH_WINDOW`.
    pub fn may_refresh_starters(&self) -> bool {
        let mut refreshes = self
            .starter_refreshes
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        while refreshes
            .front()
            .is_some_and(|at| now.duration_since(*at) >= STARTER_REFRESH_WINDOW)
        {
            refreshes.pop_front();
        }
        if refreshes.len() >= STARTER_REFRESHES {
            return false;
        }
        refreshes.push_back(now);
        true
    }

    /// The cached starters, if they still fit a phone at `here`.
    pub async fn cached_starters(&self, here: Option<&Location>) -> Option<PromptSuggestions> {
        let cache = self.starters.lock().await;
        let entry = cache.as_ref().filter(|c| c.fresh(here))?;
        Some(entry.suggestions.clone())
    }

    /// The personal starters last made, fresh or not.
    pub async fn shown_starters(&self) -> Vec<PromptSuggestion> {
        let cache = self.starters.lock().await;
        cache
            .as_ref()
            .filter(|c| c.suggestions.personal)
            .map(|c| c.suggestions.suggestions.clone())
            .unwrap_or_default()
    }

    /// Asks the model, falling back on any failure, and caches the result.
    pub async fn generate_starters(
        &self,
        generator: Generator<'_>,
        context: impl Future<Output = StarterContext>,
        here: Option<Location>,
    ) -> PromptSuggestions {
        let attempt = async {
            let context = context.await;
            generator.ask(&context).await
        };
        let (suggestions, fresh) = match tokio::time::timeout(TIMEOUT, attempt).await {
            Ok(Ok(starters)) => {
                let until_stale = starters
                    .stale_at
                    .and_then(|at| (at - Utc::now()).to_std().ok())
                    .unwrap_or(FRESH);
                (starters.suggestions, Some(until_stale.min(FRESH)))
            }
            Ok(Err(error)) => {
                log::info!("prompt suggestions fell back: {error}");
                (fallback(), None)
            }
            Err(_) => {
                log::info!("prompt suggestions fell back: timed out");
                (fallback(), None)
            }
        };
        let personal = fresh.is_some();
        let fresh = fresh.unwrap_or(FALLBACK_FRESH);
        let suggestions = PromptSuggestions {
            suggestions,
            personal,
        };
        *self.starters.lock().await = Some(Cached {
            expires: Instant::now() + fresh,
            suggestions: suggestions.clone(),
            location: here,
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

    #[tokio::test]
    async fn starters_refresh_at_most_three_times_in_ten_minutes() {
        let db = TestDb::new().await.unwrap();
        let user = User::for_test(db.store().await.unwrap(), "alice");
        for _ in 0..STARTER_REFRESHES {
            assert!(user.may_refresh_starters());
        }
        assert!(!user.may_refresh_starters());
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

    fn fact(content: &str) -> Fact<'_> {
        Fact {
            content,
            category: None,
            source: MemorySource::User {},
            valid: None,
            origin: None,
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

    #[tokio::test]
    async fn a_memory_keeps_where_it_came_from() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = User::for_test(store, "alice");
        let origin = Origin {
            reference: "email-1".into(),
            quote: Some("see you in Lisbon".into()),
        };
        let stored = alice
            .remember(Fact {
                origin: Some(origin.clone()),
                ..fact("Is going to Lisbon.")
            })
            .await
            .unwrap();
        // An update that names no origin keeps the one it had.
        assert!(
            alice
                .update_memory(&stored.id, fact("Is going to Lisbon with Ana."))
                .await
                .unwrap()
        );
        let rows = alice.memory_rows().await.unwrap();
        assert_eq!(rows[0].origin, Some(origin));
        let listed = alice.list_memories().await.unwrap();
        assert_eq!(listed[0].quote.as_deref(), Some("see you in Lisbon"));
    }
}
