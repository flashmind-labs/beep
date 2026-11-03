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
    apps::{Composio, mcp::KEEP_ALIVE},
    credentials::Opened,
    decision::DecisionTool,
    feeds::{self, SYNC_SWEEP, mail::Mailbox},
    images::Picture,
    memories::{
        CURATE_SWEEP, FILE_CONCURRENCY, FILE_QUESTION, FILING, MAX_FILED, file_question_for,
        languages, profile,
    },
    permissions::Rules,
    places::PlaceEvent,
    projects::model::{
        IssueHost, IssueRef, MachinePath, ProjectId, RepoName, Roles, StartRequest, TaskId,
        TaskState, Visibility, WorkerId, WorkerState,
    },
    protocol::{
        Cron, FilePart, ImageRef, Memory, PendingApproval, ProjectGroup, RouteCard, Run, SentFiles,
        SentImages, Session, ToolLabel, TurnSources, UpdatePage, Upload, WebSource,
        node::EnvReport,
    },
    research::pdf::{self, Paper, Pdf},
    starters::Cached,
    store::{
        Account, AccountKey, ActionRow, AppPermissionRow, AttentionItem, CheckoutRow, CopiedShare,
        CreateRun, Crossed, DeletedTurn, Fact, Feed, FinishedTurn, FoundSession, GrantRow, Invite,
        IssueAuthor, Join, JoinedGroup, LinkKey, ListedResearch, Location, MEMORY_CATEGORIES,
        McpServerRow, MemoryRow, Merge, MergeInto, NewAttachment, NewFile, NewGrant, NewKey,
        NewMessage, NewNode, NewPicture, NewPlace, NewPlaceRoutine, NewTask, NewThreadChat,
        NewWorker, NodeRow, OpenCounts, PicturesOf, PlaceRoutine, Posted, ProfileNote, ProfileRead,
        ProjectRow, PushDevice, ResearchRow, RunStatus, SaveGroup, SavedPlace, SavedTurn, Share,
        SkillOwner, SkillPatch, SkillRow, StartRefused, Store, StoredContext, StoredMessage,
        SyncRow, TaskRow, ThreadChatRow, TrackedIssue, TurnRecord, TurnsToDelete, WorkerRow,
    },
};
use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use flashmind::{
    core::{CancellationToken, Conversation},
    cron::CronJob,
    tools::mcp::McpRegistry,
    types::{Model, ReasoningLevel},
};
use futures::{StreamExt, stream};
use iroh::EndpointId;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt::{self, Write},
    sync::{Arc, Mutex, PoisonError, Weak},
    time::{Duration, Instant},
};
use tokio::{
    sync::{OnceCell, broadcast, watch},
    time::{Interval, MissedTickBehavior, interval},
};
use uuid::Uuid;

mod apps;
mod events;
mod mcp;
mod memories;
mod starters;

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
    pub(crate) mcp: OnceCell<McpRegistry>,
    /// The servers the user added by URL, which connect only when called.
    pub(crate) custom_mcp: OnceCell<McpRegistry>,
    /// Sign-ins to added servers under way, by their OAuth `state`.
    pub(crate) mcp_flows: Mutex<HashMap<String, mcp::Flow>>,
    pub(crate) notifications: broadcast::Sender<UserNotice>,
    /// Suggested prompts, kept for 12 hours.
    pub(crate) starters: AsyncMutex<Option<Cached>>,
    /// When the user last asked for new starters, oldest first.
    starter_refreshes: Mutex<VecDeque<Instant>>,
    /// Feeds being synced, so the hourly sweep and a new connection never
    /// read the same feed at once.
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
            mcp: OnceCell::new(),
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

    /// Starts a run for `message` in `session`. A repeated `request` returns
    /// its existing run.
    pub async fn create_run(
        &self,
        session: Uuid,
        request: &str,
        message: &str,
    ) -> Result<CreateRun> {
        self.store
            .create_run(&self.key, session, request, message)
            .await
    }

    /// Shows the prompt of `run`, just started, on every device while it runs.
    pub async fn save_prompt(&self, run: &Run, prompt: &str) -> Result<()> {
        self.store.save_prompt(&self.key, run, prompt).await?;
        self.changed(&run.session_id);
        Ok(())
    }

    /// The run going on in `session`, if any.
    pub async fn active_run(&self, session: Uuid) -> Result<Option<Run>> {
        self.store.active_run(&self.key, session).await
    }

    /// The runs going on in any of the user's sessions.
    pub async fn active_runs(&self) -> Result<Vec<Run>> {
        self.store.active_runs(&self.key).await
    }

    /// The run already started for `request`, without starting one.
    pub async fn existing_run(
        &self,
        session: Uuid,
        request: &str,
        message: &str,
    ) -> Result<Option<CreateRun>> {
        self.store
            .run_for_request(self.key.as_str(), session, request, Some(message))
            .await
    }

    /// The run already started for `request` in `session`, whatever its
    /// message, for a spoken message not yet transcribed.
    pub(crate) async fn run_for_spoken(
        &self,
        session: Uuid,
        request: &str,
    ) -> Result<Option<CreateRun>> {
        self.store
            .run_for_request(self.key.as_str(), session, request, None)
            .await
    }

    /// The research `chat` runs, if it is one.
    pub async fn research_in(&self, chat: Uuid) -> Result<Option<ResearchRow>> {
        self.store.research_in(&self.key, chat).await
    }

    /// The user's researches, newest first.
    pub(crate) async fn researches(&self) -> Result<Vec<ListedResearch>> {
        self.store.list_researches(&self.key).await
    }

    /// The paper of the research `chat` runs, typeset as a PDF. `None` when
    /// the chat is not a research or its paper is not written yet.
    pub(crate) async fn research_pdf(&self, chat: Uuid) -> Result<Option<Pdf>> {
        let Some(research) = self.research_in(chat).await? else {
            return Ok(None);
        };
        if research.draft.trim().is_empty() {
            return Ok(None);
        }
        let sources = self.store.research_sources(research.id).await?;
        let paper = Paper {
            topic: research.topic,
            draft: research.draft,
            sources,
            date: Utc::now().date_naive(),
        };
        pdf::render(paper).await.map(Some)
    }

    /// Makes `chat` a research on `topic`; a chat that is one keeps its own.
    pub async fn start_research(&self, chat: Uuid, topic: &str) -> Result<ResearchRow> {
        self.store.start_research(&self.key, chat, topic).await
    }

    /// The run with `id`, if it is the user's.
    pub async fn run(&self, id: &str) -> Result<Option<Run>> {
        self.store.run(&self.key, id).await
    }

    /// The user's sessions, pinned first, then newest first.
    pub async fn sessions(&self) -> Result<Vec<Session>> {
        self.store.sessions(&self.key).await
    }

    /// Up to `limit` sessions with a message that holds `query`, ignoring case.
    pub async fn search_sessions(&self, query: &str, limit: i64) -> Result<Vec<FoundSession>> {
        self.store.search_sessions(&self.key, query, limit).await
    }

    /// Renames a session; `false` when the user has no such session.
    pub async fn rename_session(&self, session_id: Uuid, title: &str) -> Result<bool> {
        let renamed = self
            .store
            .rename_session(&self.key, session_id, title)
            .await?;
        self.changed("");
        Ok(renamed)
    }

    /// Deletes a session with its runs; `false` when the user has no such session.
    pub async fn delete_session(&self, session_id: Uuid) -> Result<bool> {
        let deleted = self.store.delete_session(&self.key, session_id).await?;
        self.changed("");
        Ok(deleted)
    }

    /// Deletes a session only while it shows nothing and no run is going on
    /// in it; `false` when it was kept.
    pub(crate) async fn delete_empty_session(&self, session_id: Uuid) -> Result<bool> {
        let deleted = self
            .store
            .delete_empty_session(&self.key, session_id)
            .await?;
        if deleted {
            self.changed("");
        }
        Ok(deleted)
    }

    /// Deletes sessions with their runs and returns how many existed.
    pub async fn delete_sessions(&self, ids: &[Uuid]) -> Result<u64> {
        let deleted = self.store.delete_sessions(&self.key, ids).await?;
        self.changed("");
        Ok(deleted)
    }

    /// Removes one turn of a session, starting at the user message sent at
    /// `timestamp` in milliseconds since the epoch, or that turn and the
    /// ones after it.
    pub async fn delete_turn(
        &self,
        session_id: Uuid,
        timestamp: i64,
        which: TurnsToDelete,
    ) -> Result<DeletedTurn> {
        let deleted = self
            .store
            .delete_turn(&self.key, session_id, timestamp, which)
            .await?;
        if deleted == DeletedTurn::Deleted {
            self.changed(&session_id.to_string());
        }
        Ok(deleted)
    }

    /// The ones of `ids` the user still has.
    pub async fn existing_sessions(&self, ids: &[Uuid]) -> Result<Vec<Uuid>> {
        self.store.existing_sessions(&self.key, ids).await
    }

    /// Pins or unpins a session; `false` when the user has no such session.
    pub async fn pin_session(&self, session_id: Uuid, pinned: bool) -> Result<bool> {
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
        chat: Uuid,
        name: &str,
        mime: &str,
        data: &[u8],
    ) -> Result<String> {
        let file = NewFile {
            chat,
            name,
            mime,
            data,
        };
        self.store.save_file(&self.key, file).await
    }

    /// Keeps the pictures the user sent with a message.
    pub(crate) async fn save_pictures(
        &self,
        of: PicturesOf,
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

    /// Keeps the files other than pictures the user sent with a message.
    pub(crate) async fn save_attachments(&self, of: PicturesOf, files: &[Upload]) -> Result<()> {
        let files: Vec<NewAttachment> = files
            .iter()
            .map(|file| NewAttachment {
                name: &file.name,
                mime: &file.mime,
                data: &file.data,
            })
            .collect();
        self.store.save_attachments(&self.key, of, &files).await
    }

    pub(crate) async fn mark_spoken(&self, of: PicturesOf) -> Result<()> {
        self.store.mark_spoken(&self.key, of).await
    }

    /// When each spoken prompt of a session was sent.
    pub(crate) async fn spoken_turns(&self, session_id: Uuid) -> Result<Vec<i64>> {
        self.store.spoken_turns(&self.key, session_id).await
    }

    /// The files other than pictures the user sent in a session, by turn.
    pub(crate) async fn session_files(&self, session_id: Uuid) -> Result<Vec<SentFiles>> {
        self.store.session_files(&self.key, session_id).await
    }

    /// The pictures the user sent in a session, by turn.
    pub(crate) async fn session_pictures(&self, session_id: Uuid) -> Result<Vec<SentImages>> {
        self.store.session_pictures(&self.key, session_id).await
    }

    pub(crate) async fn save_route_card(
        &self,
        chat: Uuid,
        call_id: &str,
        card: &RouteCard,
    ) -> Result<()> {
        self.store
            .save_route_card(&self.key, chat, call_id, card)
            .await
    }

    pub(crate) async fn route_card(&self, call_id: &str) -> Result<Option<RouteCard>> {
        self.store.route_card(&self.key, call_id).await
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
        session_id: Uuid,
        conversation: &Conversation,
    ) -> Result<Option<Share>> {
        self.store
            .share_session(&self.key, session_id, conversation)
            .await
    }

    /// Stops sharing `session_id`; `false` when it was not shared.
    pub async fn unshare_session(&self, session_id: Uuid) -> Result<bool> {
        self.store.unshare_session(&self.key, session_id).await
    }

    /// Copies the share with `token` into the user's chats as `session_id`.
    pub async fn copy_share(&self, token: &str, session_id: Uuid) -> Result<CopiedShare> {
        let copied = self.store.copy_share(&self.key, token, session_id).await?;
        if copied == CopiedShare::Copied {
            self.changed("");
        }
        Ok(copied)
    }

    /// Makes the group `id` with the user in it; `false` when the id is taken.
    pub async fn create_group(&self, id: Uuid, name: &str, member_name: &str) -> Result<bool> {
        self.store
            .create_group(&self.key, id, name, member_name)
            .await
    }

    /// The groups the user is in, most recently active first.
    pub async fn groups(&self) -> Result<Vec<JoinedGroup>> {
        self.store.groups(&self.key).await
    }

    /// The group `id`, if the user is in it.
    pub async fn group(&self, id: Uuid) -> Result<Option<JoinedGroup>> {
        self.store.group(&self.key, id).await
    }

    /// Posts to a group the user is in; `None` when they are not.
    pub async fn post_to_group(&self, message: NewMessage<'_>) -> Result<Option<Posted>> {
        self.store.post_to_group(&self.key, message).await
    }

    /// The invite token of a group the user is in, a new one with `reset`.
    pub async fn group_invite(&self, id: Uuid, reset: bool) -> Result<Option<String>> {
        self.store.group_invite(&self.key, id, reset).await
    }

    /// Joins the group behind `token`.
    pub async fn join_group(&self, token: &str, name: &str) -> Result<Join> {
        self.store.join_group(&self.key, token, name).await
    }

    /// Pins or unpins a group; `false` when the user is not in it.
    pub async fn pin_group(&self, id: Uuid, pinned: bool) -> Result<bool> {
        self.store.pin_group(&self.key, id, pinned).await
    }

    /// Lets Beep use the user's apps in a group when others ask, or stops
    /// it; `false` when the user is not in it.
    pub async fn share_group_apps(&self, id: Uuid, shared: bool) -> Result<bool> {
        self.store.share_group_apps(&self.key, id, shared).await
    }

    /// Leaves a group; `false` when the user was not in it.
    pub async fn leave_group(&self, id: Uuid) -> Result<bool> {
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

    /// Whether the user has the chat `session`.
    pub async fn owns_session(&self, session: Uuid) -> Result<bool> {
        self.store.owns_session(&self.key, session).await
    }

    /// The messages of a session the user owns, oldest first. `None` when
    /// they have no such session.
    pub async fn session_messages(&self, session: Uuid) -> Result<Option<Vec<StoredMessage>>> {
        if !self.store.owns_session(&self.key, session).await? {
            return Ok(None);
        }
        Ok(Some(self.store.chat_messages(session).await?))
    }

    /// What each tool step of a session the user owns showed while it ran,
    /// oldest first. `None` when they have no such session.
    pub async fn tool_labels(&self, session: Uuid) -> Result<Option<Vec<ToolLabel>>> {
        if !self.store.owns_session(&self.key, session).await? {
            return Ok(None);
        }
        Ok(Some(self.store.tool_labels(session).await?))
    }

    /// Keeps the web pages the reply of the turn opened by message `turn`
    /// used, after any it has.
    pub(crate) async fn save_sources(
        &self,
        session: Uuid,
        turn: i64,
        sources: &[WebSource],
    ) -> Result<()> {
        self.store.save_sources(session, turn, sources).await
    }

    /// The web pages each turn of a session the user owns used. `None` when
    /// they have no such session.
    pub async fn session_sources(&self, session: Uuid) -> Result<Option<Vec<TurnSources>>> {
        if !self.store.owns_session(&self.key, session).await? {
            return Ok(None);
        }
        Ok(Some(self.store.session_sources(session).await?))
    }

    /// What the newest turn of a session ran with.
    pub async fn last_turn(&self, session: Uuid) -> Result<Option<TurnRecord>> {
        self.store.last_turn(session).await
    }

    /// What the model sees for a session.
    pub async fn context(&self, session: Uuid) -> Result<Option<StoredContext>> {
        self.store.context(&self.key, session).await
    }

    /// Whether the user is in chat `id`.
    pub async fn in_chat(&self, id: Uuid) -> Result<bool> {
        self.store.in_chat(&self.key, id).await
    }

    /// The user's updates after `after`, a page at a time.
    pub async fn updates_page(&self, after: i64) -> Result<UpdatePage> {
        self.store.updates_page(&self.key, after).await
    }

    /// The user's newest update id, now and as it moves.
    pub async fn watch_updates(&self) -> Result<watch::Receiver<i64>> {
        self.store.watch_updates(self.key.as_str()).await
    }

    /// Ends a run with `status`, saving its turn when there is one. `None`
    /// when no turn was saved.
    pub async fn finish(
        &self,
        run: &Run,
        turn: Option<FinishedTurn<'_>>,
        status: RunStatus,
    ) -> Result<Option<SavedTurn>> {
        let saved = self.store.finish(&self.key, run, turn, status).await?;
        if saved.is_some() {
            self.changed(&run.session_id);
        }
        Ok(saved)
    }

    /// Stores a new memory, filed by Jev unless it comes with a category,
    /// and returns its id.
    pub async fn add_memory(&self, fact: Fact<'_>) -> Result<String> {
        let fact = Fact {
            category: self.category_of(&fact).await,
            ..fact
        };
        self.store.add_memory(&self.key, fact).await
    }

    /// Replaces a memory's text, and its days when given, and files it again
    /// unless it comes with a category. A memory the user stated stays
    /// stated. Returns whether it exists.
    pub async fn update_memory(&self, id: &str, fact: Fact<'_>) -> Result<bool> {
        let fact = Fact {
            category: self.category_of(&fact).await,
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

    /// The category `fact` came with, or else Jev's.
    async fn category_of<'a>(&self, fact: &Fact<'a>) -> Option<&'a str> {
        match fact.category {
            Some(category) => Some(category),
            None => self.file(fact.content).await,
        }
    }

    /// Jev's category for each of `contents`, asked in one request. `None`
    /// for each when Jev is not set up or failed.
    async fn file_each(&self, contents: &[&str]) -> Vec<Option<&'static str>> {
        let Some(decision) = self.decision.as_ref() else {
            return vec![None; contents.len()];
        };
        match contents {
            [] => return Vec::new(),
            [content] => return vec![self.file(content).await],
            _ => {}
        }
        let mut state = String::from("Memories about the user:");
        for (number, content) in contents.iter().enumerate() {
            let _ = write!(state, "\n{}. {content}", number + 1);
        }
        let keys: Vec<String> = (1..=contents.len()).map(|n| n.to_string()).collect();
        let asked: Vec<String> = keys.iter().map(|key| file_question_for(key)).collect();
        let questions: Vec<(&str, &str)> = keys
            .iter()
            .zip(&asked)
            .map(|(key, question)| (key.as_str(), question.as_str()))
            .collect();
        match decision.choose_each(&state, &questions, &FILING).await {
            Ok(filed) => keys.iter().map(|key| filed.get(key).copied()).collect(),
            Err(error) => {
                log::warn!("unable to file memories: {error}; user={self}");
                vec![None; contents.len()]
            }
        }
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

    /// The memory saying which languages the user speaks, if there is one.
    pub async fn spoken_languages(&self) -> Result<Option<String>> {
        let rows = self.memory_rows().await?;
        Ok(languages(&rows, self.today().await).map(str::to_owned))
    }

    /// Deletes a memory; `false` when the user has no memory with `id`.
    pub async fn forget(&self, id: &str) -> Result<bool> {
        self.store.forget(&self.key, id).await
    }

    /// Deletes every memory of a user and returns how many there were.
    pub async fn forget_all(&self) -> Result<u64> {
        self.store.forget_all(&self.key).await
    }

    /// The user's own skills, as Beep reads and learns them.
    pub fn skill_owner(&self) -> SkillOwner {
        SkillOwner::User(self.key.clone())
    }

    /// The user's skills and their groups', for the Skills screen.
    pub async fn visible_skills(&self) -> Result<Vec<SkillRow>> {
        self.store.visible_skills(&self.key).await
    }

    /// Changes one of the skills the user can see; `None` when there is no
    /// such skill for them.
    pub async fn edit_skill(&self, id: Uuid, change: SkillPatch<'_>) -> Result<Option<SkillRow>> {
        self.store.edit_skill(&self.key, id, change).await
    }

    pub async fn delete_skill(&self, id: Uuid) -> Result<bool> {
        self.store.delete_skill(&self.key, id).await
    }

    /// Puts back the version before the skill's last change.
    pub async fn undo_skill(&self, id: Uuid) -> Result<Option<SkillRow>> {
        self.store.undo_skill(&self.key, id).await
    }

    /// Every memory for the Memories screen, most recently changed first.
    pub async fn list_memories(&self) -> Result<Vec<Memory>> {
        self.store.list_memories(&self.key).await
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

    /// Saves a grant the user approved and returns its id.
    pub(crate) async fn create_grant(&self, grant: NewGrant<'_>) -> Result<Uuid> {
        self.store.create_grant(&self.key, grant).await
    }

    /// The user's grants in force, newest first.
    pub(crate) async fn live_grants(&self) -> Result<Vec<GrantRow>> {
        self.store.live_grants(&self.key).await
    }

    /// The grants in force for calls of `tool` in runs of `routine`.
    pub(crate) async fn routine_grants(&self, routine: Uuid, tool: &str) -> Result<Vec<GrantRow>> {
        self.store.routine_grants(&self.key, routine, tool).await
    }

    /// Records a call of run `run` as a use of grant `id`; false when the
    /// grant is no longer in force or the run used its share.
    pub(crate) async fn use_grant(&self, id: Uuid, run: &str, call_id: &str) -> Result<bool> {
        self.store.use_grant(&self.key, id, run, call_id).await
    }

    /// Ends one of the user's grants; false when they have none by `id`.
    pub(crate) async fn revoke_grant(&self, id: Uuid) -> Result<bool> {
        self.store.revoke_grant(&self.key, id).await
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

    /// Keeps a new setup token's hash; returns when it expires.
    pub async fn create_node_token(
        &self,
        hash: &str,
        name: Option<&str>,
        ttl: Duration,
    ) -> Result<DateTime<Utc>> {
        self.store
            .create_node_token(&self.key, hash, name, ttl)
            .await
    }

    /// The key that claimed this user's setup token `hash`: `None` for no
    /// such token, `Some(None)` while unused.
    pub async fn node_token_claimer(&self, hash: &str) -> Result<Option<Option<String>>> {
        self.store.node_token_claimer(&self.key, hash).await
    }

    /// Stops this user's unused setup token `hash` from working; false
    /// when there is none.
    pub async fn revoke_node_token(&self, hash: &str) -> Result<bool> {
        self.store.revoke_node_token(&self.key, hash).await
    }

    /// Records that a node said hello.
    pub async fn node_connected(&self, node: NewNode<'_>) -> Result<()> {
        self.store.node_connected(&self.key, node).await
    }

    /// Stores what a node reported about its machine.
    pub async fn save_node_env(&self, node: &EndpointId, env: &EnvReport) -> Result<()> {
        self.store.save_node_env(&self.key, node, env).await
    }

    pub async fn nodes(&self) -> Result<Vec<NodeRow>> {
        self.store.nodes(&self.key).await
    }

    /// The user's settings for the coding agent harnesses on `machine_id`.
    pub(crate) async fn harness_settings(
        &self,
        machine_id: &str,
    ) -> Result<Vec<crate::store::HarnessRow>> {
        self.store.harness_settings(&self.key, machine_id).await
    }

    /// The env vars of the harness `agent` on `machine_id`, opened.
    pub(crate) async fn harness_env(
        &self,
        key: &crate::credentials::CredentialKey,
        machine_id: &str,
        agent: &str,
    ) -> Result<Vec<crate::protocol::node::EnvSecret>> {
        self.store
            .harness_env(&self.key, key, machine_id, agent)
            .await
    }

    /// Saves the user's settings for one harness on one machine.
    pub(crate) async fn save_harness_settings(
        &self,
        key: &crate::credentials::CredentialKey,
        save: crate::store::SaveHarness<'_>,
    ) -> Result<()> {
        self.store.save_harness_settings(&self.key, key, save).await
    }

    /// The harnesses the user turned off, on any machine.
    pub(crate) async fn off_harnesses(&self) -> Result<Vec<crate::store::OffHarness>> {
        self.store.off_harnesses(&self.key).await
    }

    /// The user's settings for the harnesses on `machine_id`, as the node
    /// takes them. A harness whose env vars no longer open goes without
    /// them.
    pub(crate) async fn agent_settings(
        &self,
        key: &crate::credentials::CredentialKey,
        machine_id: &str,
    ) -> Result<crate::protocol::node::harness::AgentSettings> {
        use crate::protocol::node::harness::{AgentSettings, HarnessConfig};
        let mut harnesses = Vec::new();
        for row in self.harness_settings(machine_id).await? {
            let env = if row.env_names.is_empty() {
                Vec::new()
            } else {
                match self.harness_env(key, machine_id, &row.agent).await {
                    Ok(env) => env,
                    Err(e) => {
                        log::warn!(
                            "harness env vars not opened; agent={} error={e:#}",
                            row.agent
                        );
                        Vec::new()
                    }
                }
            };
            let text = |value: String| Some(value).filter(|value| !value.is_empty());
            harnesses.push(HarnessConfig {
                agent: row.agent,
                enabled: row.enabled,
                program: text(row.program),
                home: text(row.home),
                args: row.args,
                env,
            });
        }
        Ok(AgentSettings { harnesses })
    }

    /// The machines that sync each project, by project.
    pub async fn project_syncs(&self) -> Result<Vec<SyncRow>> {
        self.store.project_syncs(&self.key).await
    }

    /// Syncs a project on a machine, in a folder.
    pub async fn set_project_sync(&self, row: &SyncRow) -> Result<()> {
        self.store.set_project_sync(&self.key, row).await
    }

    /// Stops syncing `project` on `endpoint`; `false` when it did not.
    pub async fn remove_project_sync(&self, project: &str, endpoint: &str) -> Result<bool> {
        self.store
            .remove_project_sync(&self.key, project, endpoint)
            .await
    }

    /// Records a project imported onto a machine.
    pub async fn save_import(
        &self,
        import: crate::store::NewImport<'_>,
    ) -> Result<crate::store::ImportRow> {
        self.store.save_import(&self.key, import).await
    }

    /// The projects the user imported, by project, then machine.
    pub async fn imports(&self) -> Result<Vec<crate::store::ImportRow>> {
        self.store.imports(&self.key).await
    }

    /// Deletes every row of the project `name`, the user's chats about it
    /// included; `None` when the user has no such project. See
    /// `crate::imports::remove_project`, which also stops what runs.
    pub(crate) async fn remove_project_rows(
        &self,
        name: &str,
    ) -> Result<Option<crate::store::RemovedProject>> {
        let removed = self.store.remove_project(&self.key, name).await?;
        if removed.is_some() {
            self.changed("");
        }
        Ok(removed)
    }

    /// Forgets `folder` on `endpoint` as a place of the project `name`.
    pub(crate) async fn drop_project_folder(
        &self,
        name: &str,
        endpoint: &str,
        folder: &str,
    ) -> Result<crate::store::DroppedMachine> {
        self.store
            .drop_project_folder(&self.key, name, endpoint, folder)
            .await
    }

    /// Records a checkout found on a machine as the project's folder there,
    /// unless the project has one there already; `true` when recorded.
    pub(crate) async fn link_import(&self, import: crate::store::NewImport<'_>) -> Result<bool> {
        self.store.link_import(&self.key, import).await
    }

    /// Notes when `endpoint` was looked through for checkouts.
    pub(crate) async fn save_checkout_check(
        &self,
        endpoint: &str,
        at: DateTime<Utc>,
    ) -> Result<()> {
        self.store
            .save_checkout_check(&self.key, endpoint, at)
            .await
    }

    /// When each machine was last looked through for checkouts, by key.
    pub(crate) async fn checkout_checks(&self) -> Result<HashMap<String, DateTime<Utc>>> {
        self.store.checkout_checks(&self.key).await
    }

    /// The user's git credentials (GitHub, Forgejo servers), opened.
    pub async fn git_credentials(
        &self,
        key: &crate::credentials::CredentialKey,
    ) -> Result<Vec<Opened>> {
        let opened = self.store.machine_credentials(&self.key, key).await?;
        Ok(opened
            .into_iter()
            .filter(|opened| opened.slot.always_to_machines())
            .collect())
    }

    pub async fn pinned_projects(&self) -> Result<Vec<String>> {
        self.store.pinned_projects(&self.key).await
    }

    /// Pins `project`, which stops it on every machine, or unpins it.
    pub async fn pin_project(&self, project: &str, pinned: bool) -> Result<()> {
        self.store.pin_project(&self.key, project, pinned).await
    }

    /// The projects whose sync is paused.
    pub async fn paused_syncs(&self) -> Result<Vec<String>> {
        self.store.paused_syncs(&self.key).await
    }

    /// Pauses `project`'s sync, or resumes it; `false` when it already was.
    pub async fn pause_sync(&self, project: &str, paused: bool) -> Result<bool> {
        self.store.pause_sync(&self.key, project, paused).await
    }

    /// Lets `allow` run without asking for the rest of `chat`.
    pub(crate) async fn allow_in_chat(
        &self,
        chat: Uuid,
        allow: &crate::store::ChatAllow,
    ) -> Result<()> {
        self.store.allow_in_chat(&self.key, chat, allow).await
    }

    /// What the user allowed for the rest of `chat`.
    pub(crate) async fn chat_allows(&self, chat: Uuid) -> Result<Vec<crate::store::ChatAllow>> {
        self.store.chat_allows(&self.key, chat).await
    }

    /// `project`'s terminals, oldest first.
    pub(crate) async fn terminals(&self, project: &str) -> Result<Vec<crate::store::TerminalRow>> {
        self.store.terminals(&self.key, project).await
    }

    /// Every terminal, of any project or none, oldest first.
    pub(crate) async fn all_terminals(&self) -> Result<Vec<crate::store::TerminalRow>> {
        self.store.all_terminals(&self.key).await
    }

    pub(crate) async fn terminal(&self, id: Uuid) -> Result<Option<crate::store::TerminalRow>> {
        self.store.terminal(&self.key, id).await
    }

    pub(crate) async fn add_terminal(&self, row: &crate::store::TerminalRow) -> Result<()> {
        self.store.add_terminal(&self.key, row).await
    }

    pub(crate) async fn remove_terminal(&self, id: Uuid) -> Result<bool> {
        self.store.remove_terminal(&self.key, id).await
    }

    pub(crate) async fn add_review(
        &self,
        review: &crate::store::ReviewRow,
        shards: &[crate::store::ShardRow],
    ) -> Result<()> {
        self.store.add_review(&self.key, review, shards).await
    }

    pub(crate) async fn review(&self, id: Uuid) -> Result<Option<crate::store::ReviewRow>> {
        self.store.review(&self.key, id).await
    }

    /// `project`'s newest review of each branch, newest first.
    pub(crate) async fn reviews(
        &self,
        project: &str,
        limit: u32,
    ) -> Result<Vec<crate::store::ReviewRow>> {
        self.store.reviews(&self.key, project, limit).await
    }

    /// The shards of one of the user's reviews, read through `review`.
    pub(crate) async fn review_shards(
        &self,
        review: &crate::store::ReviewRow,
    ) -> Result<Vec<crate::store::ShardRow>> {
        self.store.review_shards(review.id).await
    }

    pub(crate) async fn review_findings(
        &self,
        review: &crate::store::ReviewRow,
    ) -> Result<Vec<crate::store::ReviewFindingRow>> {
        self.store.review_findings(review.id).await
    }

    pub(crate) async fn report_shard(
        &self,
        review: &crate::store::ReviewRow,
        shard: u32,
        findings: &[crate::store::ReviewFindingRow],
        next: crate::store::ShardState,
    ) -> Result<bool> {
        self.store
            .report_shard(review.id, shard, findings, next)
            .await
    }

    pub(crate) async fn judge_shard(
        &self,
        review: &crate::store::ReviewRow,
        shard: u32,
        judged: &[crate::store::Judged],
    ) -> Result<bool> {
        self.store.judge_shard(review.id, shard, judged).await
    }

    pub(crate) async fn fail_shard(
        &self,
        review: &crate::store::ReviewRow,
        shard: u32,
        from: crate::store::ShardState,
        why: &str,
    ) -> Result<bool> {
        self.store.fail_shard(review.id, shard, from, why).await
    }

    pub(crate) async fn dismiss_finding(
        &self,
        review: Uuid,
        finding: u32,
        dismissed: bool,
    ) -> Result<bool> {
        self.store
            .dismiss_finding(&self.key, review, finding, dismissed)
            .await
    }

    /// The newest `limit` versions of the user's instructions, newest
    /// first.
    pub(crate) async fn instructions_versions(
        &self,
        limit: i64,
    ) -> Result<Vec<crate::store::InstructionsRow>> {
        self.store.instructions_versions(&self.key, limit).await
    }

    /// Saves the user's instructions as the version after `base`.
    pub(crate) async fn save_instructions(
        &self,
        base: Option<u32>,
        text: &str,
        note: &str,
    ) -> Result<crate::store::Saved> {
        self.store
            .save_instructions(&self.key, base, text, note)
            .await
    }

    /// The projects pinned to the top of the sidebar, oldest pin first.
    pub async fn projects_on_top(&self) -> Result<Vec<String>> {
        self.store.projects_on_top(&self.key).await
    }

    /// Pins `project` to the top of the sidebar, or unpins it.
    pub async fn pin_project_to_top(&self, project: &str, pinned: bool) -> Result<()> {
        self.store
            .pin_project_to_top(&self.key, project, pinned)
            .await
    }

    /// The user's project groups, oldest first.
    pub async fn project_groups(&self) -> Result<Vec<ProjectGroup>> {
        self.store.project_groups(&self.key).await
    }

    /// Makes a project group, or changes the user's group `group.id`.
    /// `None` when there is no such group.
    pub(crate) async fn save_project_group(
        &self,
        group: SaveGroup<'_>,
    ) -> Result<Option<ProjectGroup>> {
        self.store.save_project_group(&self.key, group).await
    }

    /// Forgets the user's project group `id`; false when there is none.
    pub async fn delete_project_group(&self, id: Uuid) -> Result<bool> {
        self.store.delete_project_group(&self.key, id).await
    }

    /// The user's API keys, opened.
    pub(crate) async fn api_keys(
        &self,
        key: &crate::credentials::CredentialKey,
    ) -> Result<Vec<crate::store::ApiKey>> {
        self.store.api_keys(&self.key, key).await
    }

    /// The user's credentials, without their secrets.
    pub async fn credentials(
        &self,
        key: &crate::credentials::CredentialKey,
    ) -> Result<Vec<crate::store::CredentialRow>> {
        self.store.credentials(&self.key, key).await
    }

    /// Seals and saves a credential, replacing the one in its slot.
    pub async fn save_credential(
        &self,
        key: &crate::credentials::CredentialKey,
        new: crate::store::NewCredential<'_>,
    ) -> Result<crate::store::CredentialRow> {
        self.store.save_credential(&self.key, key, new).await
    }

    /// Seals and saves a GitHub token as the account it signs in as.
    pub async fn save_github_account(
        &self,
        key: &crate::credentials::CredentialKey,
        new: crate::store::NewGitHubAccount<'_>,
    ) -> Result<crate::store::GitHubAccountRow> {
        self.store.save_github_account(&self.key, key, new).await
    }

    /// The user's GitHub accounts, the default first.
    pub async fn github_accounts(
        &self,
        key: &crate::credentials::CredentialKey,
    ) -> Result<Vec<crate::store::GitHubAccountRow>> {
        self.store.github_accounts(&self.key, key).await
    }

    /// `false` when the user has no GitHub account `id`.
    pub async fn set_default_github(&self, id: &str) -> Result<bool> {
        self.store.set_default_github(&self.key, id).await
    }

    /// The user's coding agents' tokens, without their secrets.
    pub(crate) async fn agent_tokens(
        &self,
        key: &crate::credentials::CredentialKey,
    ) -> Result<Vec<crate::store::AgentTokenRow>> {
        self.store.agent_tokens(&self.key, key).await
    }

    /// `false` when the user has no token for `agent`.
    pub(crate) async fn remove_agent_token(
        &self,
        agent: crate::protocol::AgentAccount,
    ) -> Result<bool> {
        self.store.remove_agent_token(&self.key, agent).await
    }

    /// `false` when the user has no credential `id`.
    pub async fn remove_credential(&self, id: &str) -> Result<bool> {
        self.store.remove_credential(&self.key, id).await
    }

    /// The user's git host tokens, opened.
    pub async fn git_logins(
        &self,
        key: &crate::credentials::CredentialKey,
    ) -> Result<Vec<crate::store::GitLogin>> {
        self.store.git_logins(&self.key, key).await
    }

    /// The user's credentials for their machines, opened.
    pub async fn machine_credentials(
        &self,
        key: &crate::credentials::CredentialKey,
    ) -> Result<crate::protocol::node::MachineCredentials> {
        let opened = self.store.machine_credentials(&self.key, key).await?;
        Ok(crate::credentials::for_machines(opened))
    }

    /// The account's primary key, then the keys linked to it.
    pub async fn account_keys(&self) -> Result<Vec<AccountKey>> {
        self.store.account_keys(&self.key).await
    }

    /// A code another device joins this account with, shown by `endpoint`
    /// under `name`.
    pub async fn create_invite(
        &self,
        endpoint: &EndpointId,
        name: &str,
        ttl: Duration,
    ) -> Result<Invite> {
        self.store
            .create_invite(&self.key, endpoint, name, ttl)
            .await
    }

    /// Moves this account into the one that made the code.
    pub async fn merge_into(&self, into: MergeInto<'_>) -> Result<Merge> {
        self.store.merge_into(&self.key, into).await
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

    /// Records that the phone saw itself arrive at or leave place `id`, and
    /// returns the crossing when that changed where it was.
    pub async fn cross_place(&self, id: Uuid, event: PlaceEvent) -> Result<Option<Crossed>> {
        self.store
            .cross_place(&self.key, id, event, Utc::now())
            .await
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

/// The user's projects, their tasks and workers.
#[allow(dead_code, reason = "the project agent uses these in the next tasks")]
impl User {
    pub(crate) async fn ensure_project(&self, name: &RepoName) -> Result<ProjectRow> {
        self.store.ensure_project(&self.key, name).await
    }

    pub(crate) async fn projects(&self) -> Result<Vec<ProjectRow>> {
        self.store.projects(&self.key).await
    }

    pub(crate) async fn project(&self, id: ProjectId) -> Result<Option<ProjectRow>> {
        self.store.project(&self.key, id).await
    }

    /// The project whose agent talks in `chat`, if it is one.
    pub(crate) async fn project_in(&self, chat: Uuid) -> Result<Option<ProjectRow>> {
        self.store.project_in(&self.key, chat).await
    }

    /// The chat of the project's agent, made on first use.
    pub(crate) async fn ensure_project_agent(&self, project: ProjectId) -> Result<Uuid> {
        self.store.ensure_project_agent(&self.key, project).await
    }

    /// How many of the project's tasks are open and workers live.
    /// Every project summed up for the Projects overview, by name.
    pub(crate) async fn project_overviews(&self) -> Result<Vec<crate::store::OverviewRow>> {
        self.store.project_overviews(&self.key).await
    }

    pub(crate) async fn project_open_counts(&self, project: ProjectId) -> Result<OpenCounts> {
        self.store.open_counts(project).await
    }

    /// The chats of the project agents made so far.
    pub(crate) async fn project_chats(&self) -> Result<Vec<Uuid>> {
        self.store.project_chats(&self.key).await
    }

    /// The cards `chats` wait on, with their chats.
    pub(crate) async fn open_approvals(
        &self,
        chats: &[Uuid],
    ) -> Result<Vec<(Uuid, PendingApproval)>> {
        self.store.open_approvals(&self.key, chats).await
    }

    pub(crate) async fn project_task(&self, id: TaskId) -> Result<Option<TaskRow>> {
        self.store.task(&self.key, id).await
    }

    /// Tasks waiting for a slot to start, oldest first.
    pub(crate) async fn queued_tasks(&self) -> Result<Vec<TaskRow>> {
        self.store.queued_tasks(&self.key).await
    }

    pub(crate) async fn start_worker(
        &self,
        worker: &NewWorker,
    ) -> Result<Result<WorkerRow, StartRefused>> {
        self.store.start_worker(&self.key, worker).await
    }

    pub(crate) async fn live_workers_on(&self, machine: &EndpointId) -> Result<Vec<WorkerRow>> {
        self.store.live_workers_on(&self.key, machine).await
    }

    pub(crate) async fn live_workers(&self) -> Result<Vec<WorkerRow>> {
        self.store.live_workers(&self.key).await
    }

    pub(crate) async fn project_worker(&self, id: WorkerId) -> Result<Option<WorkerRow>> {
        self.store.worker(&self.key, id).await
    }

    // The calls below take a project or task id the caller already read
    // through one of the user's own lookups above.

    pub(crate) async fn checkouts(&self, project: ProjectId) -> Result<Vec<CheckoutRow>> {
        self.store.checkouts(project).await
    }

    pub(crate) async fn add_checkout(
        &self,
        project: ProjectId,
        checkout: &CheckoutRow,
    ) -> Result<()> {
        self.store.add_checkout(project, checkout).await
    }

    pub(crate) async fn set_home(
        &self,
        project: ProjectId,
        machine: &EndpointId,
        path: &MachinePath,
    ) -> Result<()> {
        self.store.set_home(project, machine, path).await
    }

    pub(crate) async fn set_pin(&self, project: ProjectId, pin: Option<&EndpointId>) -> Result<()> {
        self.store.set_pin(project, pin).await
    }

    pub(crate) async fn set_project_setup(
        &self,
        project: ProjectId,
        roles: &Roles,
        visibility: Visibility,
        default_branch: &str,
    ) -> Result<()> {
        self.store
            .set_project_setup(project, roles, visibility, default_branch)
            .await
    }

    pub(crate) async fn create_tasks(
        &self,
        project: ProjectId,
        tasks: &[NewTask],
    ) -> Result<Vec<TaskRow>> {
        self.store.create_tasks(project, tasks).await
    }

    pub(crate) async fn track_issue(
        &self,
        project: ProjectId,
        task: &NewTask,
        host: &IssueHost,
        issue: &IssueRef,
        author: IssueAuthor,
    ) -> Result<TrackedIssue> {
        self.store
            .track_issue(project, task, host, issue, author)
            .await
    }

    pub(crate) async fn dismiss_issue(
        &self,
        project: ProjectId,
        host: &IssueHost,
        number: u64,
    ) -> Result<()> {
        self.store.dismiss_issue(project, host, number).await
    }

    pub(crate) async fn dismissed_issues(
        &self,
        project: ProjectId,
        host: &IssueHost,
    ) -> Result<Vec<u64>> {
        self.store.dismissed_issues(project, host).await
    }

    pub(crate) async fn note_issue_comment(&self, task: TaskId, once: bool) -> Result<bool> {
        self.store.note_issue_comment(task, once).await
    }

    /// The chat about one thread of `project`, made on the first ask.
    pub(crate) async fn open_thread_chat(
        &self,
        project: ProjectId,
        new: &NewThreadChat<'_>,
    ) -> Result<ThreadChatRow> {
        self.store.open_thread_chat(&self.key, project, new).await
    }

    pub(crate) async fn thread_chats(&self, project: ProjectId) -> Result<Vec<ThreadChatRow>> {
        self.store.thread_chats(project).await
    }

    /// The thread `chat` is about, when it is a thread chat.
    pub(crate) async fn thread_in(&self, chat: Uuid) -> Result<Option<ThreadChatRow>> {
        self.store.thread_in(&self.key, chat).await
    }

    pub(crate) async fn note_thread_reply(
        &self,
        project: ProjectId,
        repo: &str,
        number: u64,
    ) -> Result<bool> {
        self.store.note_thread_reply(project, repo, number).await
    }

    pub(crate) async fn project_tasks(&self, project: ProjectId) -> Result<Vec<TaskRow>> {
        self.store.tasks(project).await
    }

    pub(crate) async fn task_by_slug(
        &self,
        project: ProjectId,
        slug: &str,
    ) -> Result<Option<TaskRow>> {
        self.store.task_by_slug(project, slug).await
    }

    pub(crate) async fn set_task_state(&self, task: TaskId, state: TaskState) -> Result<()> {
        self.store.set_task_state(task, state).await
    }

    /// Moves the task from `from` to `to` unless it moved since it was read.
    pub(crate) async fn move_task_state(
        &self,
        task: TaskId,
        from: TaskState,
        to: TaskState,
    ) -> Result<bool> {
        self.store.move_task_state(task, from, to).await
    }

    pub(crate) async fn set_task_issue(
        &self,
        task: TaskId,
        host: &IssueHost,
        issue: &IssueRef,
    ) -> Result<()> {
        self.store.set_task_issue(task, host, issue).await
    }

    pub(crate) async fn set_task_start(
        &self,
        task: TaskId,
        start: Option<&StartRequest>,
    ) -> Result<()> {
        self.store.set_task_start(task, start).await
    }

    pub(crate) async fn set_task_branch(&self, task: TaskId, branch: &str) -> Result<()> {
        self.store.set_task_branch(task, branch).await
    }

    pub(crate) async fn set_task_pr(&self, task: TaskId, url: &str) -> Result<()> {
        self.store.set_task_pr(task, url).await
    }

    pub(crate) async fn set_task_after(&self, task: TaskId, after: Option<TaskId>) -> Result<()> {
        self.store.set_task_after(task, after).await
    }

    pub(crate) async fn update_task_text(
        &self,
        task: TaskId,
        title: Option<&str>,
        body: Option<&str>,
    ) -> Result<()> {
        self.store.update_task_text(task, title, body).await
    }

    pub(crate) async fn set_worker_state(
        &self,
        worker: WorkerId,
        state: WorkerState,
    ) -> Result<bool> {
        self.store.set_worker_state(worker, state).await
    }

    pub(crate) async fn worker_turn(
        &self,
        worker: WorkerId,
        turns: u32,
        last_output: &str,
        at: DateTime<Utc>,
    ) -> Result<bool> {
        self.store.worker_turn(worker, turns, last_output, at).await
    }

    pub(crate) async fn set_worker_asking(&self, worker: WorkerId, asking: bool) -> Result<()> {
        self.store.set_worker_asking(worker, asking).await
    }

    pub(crate) async fn set_pr_seen(&self, task: TaskId, fingerprint: &str) -> Result<()> {
        self.store.set_pr_seen(task, fingerprint).await
    }

    /// The project's live workers and those that ended since `since`.
    pub(crate) async fn project_workers(
        &self,
        project: ProjectId,
        since: DateTime<Utc>,
    ) -> Result<Vec<WorkerRow>> {
        self.store.workers(project, since).await
    }

    pub(crate) async fn set_worker_session(&self, worker: WorkerId, session: &str) -> Result<()> {
        self.store.set_worker_session(worker, session).await
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
