//! The hub: `App` holds the server's shared state; sign-in is in `admission`, turns start in
//! `submit` and run in `run`.

pub mod admin;
mod admission;
mod auto;
mod context;
mod device;
mod group;
mod group_reminders;
mod group_tools;
mod learning;
pub(crate) mod memory_context;
mod notice;
mod notify;
mod research;
mod run;
mod skills;
mod starters;
mod submit;
mod voice;

pub use notice::{Answer, Draws, Message, Reach, UserNotice};

pub(crate) use group::group_message;

use crate::{
    AsyncMutex,
    account::credentials::CredentialKey,
    account::user::{Shared, User},
    account::{
        instance::{Instance, InstanceConfig},
        login::LoginConfig,
    },
    agent::compaction::{self, Summarizer},
    agent::decision::DecisionTool,
    agent::images::Describer,
    agent::live::LiveRun,
    agent::models::ModelChoices,
    agent::titles,
    agent::tool_attachments::RecentAttachments,
    apps::{Composio, ComposioSettings},
    memory::starters::Generator,
    protocol::DeviceKind,
    store::{Account, SessionRef, Store},
    tools::Transitous,
    tools::search::SearchTool,
    voice::stt::{RealtimeStt, Unavailable},
    voice::{Voice, VoiceConfig},
};
use anyhow::Result;
use flashmind::{
    core::CancellationToken,
    types::{AgentLlmConfig, LlmProvider, Model, ReasoningLevel},
};
use secrecy::SecretString;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, PoisonError, atomic::AtomicBool},
    time::Duration,
};
use tokio::sync::broadcast;
use uuid::Uuid;

/// How long a stop waits for its run to save.
const STOPPING_WAIT: Duration = Duration::from_secs(10);

/// The server's shared state: storage, users, models and integrations.
pub struct App {
    pub store: Store,
    /// Notices for the phones; each stream keeps only its user's.
    pub notifications: broadcast::Sender<UserNotice>,
    /// The model behind runs, titles and starters.
    provider: Arc<dyn LlmProvider>,
    /// Model, reasoning and token budget for every run; each run adds its user.
    llm: AgentLlmConfig,
    /// The optional tools each run gets.
    tools: RunTools,
    /// Describes images; without it only a model that sees them can.
    vision_model: Option<Model>,
    /// What users may run their own turns on.
    pub models: ModelChoices,
    /// What a new `User` is built from.
    shared: Shared,
    /// Every user signed in since the server started, by id.
    users: Mutex<HashMap<String, Arc<User>>>,
    /// Runs in progress, with their event feed.
    runs: AsyncMutex<HashMap<String, Arc<LiveRun>>>,
    /// Questions sent to a phone, keyed by query id, waiting for its answer.
    device_queries: device::DeviceQueries,
    /// Groups Beep is writing a reply in.
    groups_busy: Mutex<HashSet<Uuid>>,
    /// Actions Beep asked a group member to allow, keyed by query id.
    group_actions: Mutex<HashMap<String, group_tools::GroupAction>>,
    /// Set once the server starts stopping.
    shutting_down: AtomicBool,
    /// Set once the server stops taking prompts, to let running turns end.
    draining: AtomicBool,
    /// Research teams at work, by research id.
    teams: AsyncMutex<HashMap<Uuid, CancellationToken>>,
    /// The nodes connected now.
    pub nodes: crate::nodes::Nodes,
    /// The project workers' sessions and the issue mirroring in flight.
    pub projects: crate::projects::Projects,
    /// Looks through the users' machines for checkouts of their projects.
    pub checkouts: crate::projects::imports::Checkouts,
    /// Speaks voice notes and transcribes spoken replies.
    voice: Voice,
    /// Transcribes speech live, for `Transcribe`.
    realtime: Arc<dyn RealtimeStt>,
    /// Live transcriptions running at once, server-wide.
    live_transcriptions: voice::LiveSlots,
    /// Account events on their way to the agent (see `crate::routines::events`).
    pub events: crate::routines::events::Hub,
    /// Sub-agent runs outside the chats, such as event triage.
    pub subagents: crate::agent::subagents::Subagents,
    /// Set when the server runs for one owner (see `crate::account::instance`).
    instance: Option<Instance>,
    /// Set when keys sign in through a website (see `crate::account::login`).
    login: Option<LoginConfig>,
    /// Agents that read finished chats over (see `crate::memory::watchers`).
    pub watchers: crate::memory::watchers::Hub,
    /// Seals and opens users' credentials.
    pub credential_key: CredentialKey,
    /// Each user's open PRs and issues on their git hosts, cached a few minutes.
    pub git_items: crate::projects::git_items::GitBoard,
    /// Each user's repositories on their git hosts, cached a few minutes.
    pub host_repos: crate::projects::imports::HostRepoCache,
    /// Each project's PRs, issues and workflow runs, refreshed while someone looks.
    pub repo_watch: crate::projects::repo_watch::RepoWatch,
}

/// Optional server-credentialed tools and single-user settings, as `App::new` takes them.
#[derive(Default)]
pub struct ServerTools {
    pub search: Option<Arc<SearchTool>>,
    /// Firecrawl key for scraping, mapping, crawling and extracting pages.
    pub firecrawl_key: Option<SecretString>,
    pub decision: Option<Arc<DecisionTool>>,
    /// Composio apps the user connects from the phone.
    pub composio: Option<ComposioSettings>,
    /// Describes images; see `Config::vision_model`.
    pub vision_model: Option<Model>,
    /// Transit routes for `get_route`.
    pub transitous: Option<Arc<Transitous>>,
    /// The models voice notes use.
    pub voice: VoiceConfig,
    /// Live transcription; `None` answers `Transcribe` with `Unavailable`.
    pub realtime: Option<Arc<dyn RealtimeStt>>,
    /// Mistral, which transcribes spoken replies when set.
    pub mistral: Option<Arc<dyn LlmProvider>>,
    /// Runs the server for one owner; `None` serves everyone.
    pub instance: Option<InstanceConfig>,
    /// Hosted sign-in; `None` lets an unknown key make its own account.
    pub login: Option<LoginConfig>,
    /// Seals users' credentials; `None` makes a process-lifetime key, for tests.
    pub credential_key: Option<CredentialKey>,
}

/// The part of `ServerTools` a run registers itself.
struct RunTools {
    search: Option<Arc<SearchTool>>,
    firecrawl_key: Option<SecretString>,
    decision: Option<Arc<DecisionTool>>,
    transitous: Option<Arc<Transitous>>,
}

/// Who a connection signed in as.
pub struct SignedIn {
    pub user: Arc<User>,
    pub device: DeviceKind,
    /// The key that made the account, which may link and remove others.
    pub primary: bool,
}

/// How a connection's key may use the server.
pub enum Admission {
    SignedIn(SignedIn),
    /// The key was linked and then removed.
    Revoked,
    /// A single-user server's key that is not one of its owner's.
    NotInvited,
    /// An unlinked key that may only sign in: with a join code on a single-user server with an
    /// owner, or also through the website on a hosted one.
    Guest,
}

impl App {
    pub fn new(
        store: Store,
        provider: Arc<dyn LlmProvider>,
        llm: AgentLlmConfig,
        tools: ServerTools,
        models: ModelChoices,
    ) -> Arc<Self> {
        let ServerTools {
            search,
            firecrawl_key,
            decision,
            composio,
            vision_model,
            transitous,
            voice,
            realtime,
            mistral,
            instance,
            login,
            credential_key,
        } = tools;
        let (notifications, _) = broadcast::channel(64);
        let provider: Arc<dyn LlmProvider> = Arc::new(RecentAttachments::new(provider));

        let shared = Shared {
            store: store.clone(),
            notifications: notifications.clone(),
            composio: composio.map(|settings| Arc::new(Composio::new(settings))),
            decision: decision.clone(),
        };
        let voice = Voice::new(provider.clone(), mistral, voice);
        let subagents = crate::agent::subagents::Subagents::new(
            provider.clone(),
            llm.clone(),
            notifications.clone(),
        );
        Arc::new(Self {
            store,
            notifications,
            provider,
            llm,
            vision_model,
            models,
            tools: RunTools {
                search,
                firecrawl_key,
                decision,
                transitous,
            },
            shared,
            users: Mutex::new(HashMap::new()),
            runs: AsyncMutex::new(HashMap::new()),
            device_queries: device::DeviceQueries::default(),
            groups_busy: Mutex::new(HashSet::new()),
            group_actions: Mutex::new(HashMap::new()),
            shutting_down: AtomicBool::new(false),
            draining: AtomicBool::new(false),
            teams: AsyncMutex::new(HashMap::new()),
            nodes: crate::nodes::Nodes::default(),
            projects: crate::projects::Projects::default(),
            checkouts: crate::projects::imports::Checkouts::default(),
            voice,
            realtime: realtime.unwrap_or_else(|| Arc::new(Unavailable)),
            live_transcriptions: voice::LiveSlots::default(),
            events: crate::routines::events::Hub::default(),
            subagents,
            instance: instance.map(Instance::new),
            login,
            watchers: crate::memory::watchers::Hub::default(),
            credential_key: credential_key.unwrap_or_else(CredentialKey::random),
            git_items: crate::projects::git_items::GitBoard::new(),
            host_repos: crate::projects::imports::HostRepoCache::default(),
            repo_watch: crate::projects::repo_watch::RepoWatch::default(),
        })
    }

    /// The signed in user with `id`.
    pub fn user(&self, id: &str) -> Option<Arc<User>> {
        self.users
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .cloned()
    }

    /// Every user started so far.
    pub fn users(&self) -> Vec<Arc<User>> {
        self.users
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .cloned()
            .collect()
    }

    /// Starts every known user, so their crons and feeds run without a connection.
    pub async fn start_users(self: &Arc<Self>) -> Result<()> {
        for account in self.store.accounts().await? {
            self.start_user(&account);
        }
        Ok(())
    }

    /// After `source` merged into `target`: stops the gone account and restarts the target.
    pub async fn merged(self: &Arc<Self>, source: &str, target: &str) -> Result<()> {
        let account = self
            .store
            .account(target)
            .await?
            .ok_or_else(|| anyhow::anyhow!("account {target} is gone"))?;
        let old = {
            let mut users = self.users.lock().unwrap_or_else(PoisonError::into_inner);
            [users.remove(source), users.remove(target)]
        };
        for user in old.into_iter().flatten() {
            user.stop();
        }
        self.start_user(&account);
        Ok(())
    }

    /// The user behind `account`, started if it was not.
    pub fn user_for(self: &Arc<Self>, account: &Account) -> Arc<User> {
        self.start_user(account)
    }

    fn start_user(self: &Arc<Self>, account: &Account) -> Arc<User> {
        let id = &account.id;
        let mut users = self.users.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(user) = users.get(id) {
            return user.clone();
        }
        let user = User::new(account, &self.shared);
        users.insert(id.clone(), user.clone());
        // `start` only spawns, so holding the lock here is fine.
        user.start(self);
        log::info!("user started; user={id}");
        user
    }

    /// The model `user`'s turns run on.
    pub(crate) async fn model_for(&self, user: &User) -> Model {
        let picked = user.model().await.unwrap_or_else(|e| {
            log::warn!("reading the picked model failed; user={}: {e:#}", user.id());
            None
        });
        self.models.for_user(picked.as_deref()).clone()
    }

    /// How long the model reasons on `user`'s turns: their pick, else the config's.
    pub(crate) async fn thinking_for(&self, user: &User) -> ReasoningLevel {
        let picked = user.thinking().await.unwrap_or_else(|e| {
            log::warn!(
                "reading the picked thinking failed; user={}: {e:#}",
                user.id()
            );
            None
        });
        picked.unwrap_or(self.llm.reasoning)
    }

    /// The image describer: the configured vision model, or `own` when it sees images.
    fn describer(&self, user: &User, own: &Model, sees_images: bool) -> Option<Describer> {
        let model = self
            .vision_model
            .clone()
            .or_else(|| sees_images.then(|| own.clone()))?;
        Some(Describer {
            provider: self.provider.clone(),
            model,
            user: user.id().to_owned(),
        })
    }

    /// Summarizes sessions that are long and have gone quiet.
    pub async fn compact_idle_sessions(&self) {
        compaction::compact_idle(&self.store, self.summarizer()).await;
    }

    /// Compacts `session` in the background when its last prompt on `model` hit the limit.
    pub(super) fn compact_after_turn(
        self: &Arc<Self>,
        session: SessionRef,
        model: Model,
        prompt_tokens: u32,
    ) {
        let app = self.clone();
        tokio::spawn(async move {
            let limit = compaction::compact_at(app.provider.context_window(&model).await);
            if prompt_tokens < limit {
                return;
            }
            compaction::compact_session(&app.store, app.provider.as_ref(), &model, &session, limit)
                .await;
        });
    }

    /// Retitles sessions that changed and have gone quiet.
    pub async fn retitle_idle_sessions(&self) {
        let retitled = titles::retitle_idle(&self.store, self.provider.as_ref(), &self.llm).await;
        for session in retitled {
            let _ = self
                .notifications
                .send(UserNotice::changed(&session.user, ""));
        }
    }

    /// The Composio account, when the server has a key.
    pub(crate) fn composio(&self) -> Option<&Arc<Composio>> {
        self.shared.composio.as_ref()
    }

    /// Jev, when the server has an OpenRouter key.
    pub fn decision(&self) -> Option<&DecisionTool> {
        self.tools.decision.as_deref()
    }

    /// Summarizes each session on the model its chat ran on.
    fn summarizer(&self) -> Summarizer<'_> {
        Summarizer {
            provider: self.provider.as_ref(),
            models: &self.models,
            default: &self.llm.model,
        }
    }

    /// Plain completions on `llm` for `user`, metered to them.
    pub(crate) fn generator_on<'a>(
        &'a self,
        user: &'a str,
        llm: &'a AgentLlmConfig,
    ) -> Generator<'a> {
        Generator {
            provider: self.provider.as_ref(),
            llm,
            user,
        }
    }

    /// The assistant's model settings, such as its provider routing.
    pub fn llm(&self) -> &AgentLlmConfig {
        &self.llm
    }

    /// Plain completions on the assistant's model for `user`.
    pub fn generator<'a>(&'a self, user: &'a str) -> Generator<'a> {
        Generator {
            provider: self.provider.as_ref(),
            llm: &self.llm,
            user,
        }
    }
}

#[cfg(test)]
mod tests;
