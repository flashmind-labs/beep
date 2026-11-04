pub mod admin;
mod auto;
mod context;
mod device;
mod group;
mod group_reminders;
mod group_tools;
mod learning;
pub(crate) mod memory_context;
mod notify;
mod research;
mod run;
mod skills;
mod starters;
mod voice;

pub(crate) use group::group_message;

use crate::{
    AsyncMutex,
    account::credentials::CredentialKey,
    account::instance::{Instance, InstanceConfig},
    account::user::{Shared, User},
    agent::compaction::{self, Summarizer},
    agent::decision::DecisionTool,
    agent::images::Describer,
    agent::live::LiveRun,
    agent::metering,
    agent::models::ModelChoices,
    agent::titles,
    agent::tool_attachments::RecentAttachments,
    apps::{Composio, ComposioSettings},
    memory::starters::Generator,
    protocol::{ApprovalDecision, DeviceKind, Notice, Platform, RunEvent, Upload},
    routines::wake::Woken,
    store::{Account, CreateRun, LinkKey, NewKey, RunStatus, SessionRef, SignIn, Store},
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
use iroh::EndpointId;
use secrecy::SecretString;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::broadcast,
    time::{sleep, timeout},
};
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
    /// An unlinked key on a single-user server with an owner: it may only join with a code.
    Guest,
}

/// A `Notice` addressed to one user's open notification streams.
#[derive(Debug, Clone)]
pub struct UserNotice {
    pub user: String,
    pub notice: Notice,
    /// What the user's computers get instead of `notice`, when it differs.
    pub desktop: Option<Notice>,
    /// Which of the user's devices get it.
    pub to: Reach,
}

/// Which of a user's devices a `UserNotice` goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    Everyone,
    /// The computers only, never a phone.
    Desktops,
    /// Only the device with this key, such as one that started a coding-agent session.
    Device(EndpointId),
}

impl Reach {
    fn includes(self, id: &EndpointId, kind: &DeviceKind) -> bool {
        match self {
            Self::Everyone => true,
            Self::Desktops => matches!(kind, DeviceKind::Desktop {}),
            Self::Device(device) => device == *id,
        }
    }
}

impl UserNotice {
    /// Tells the user's phones `session_id` changed, or the chat list when empty.
    pub fn changed(user: &str, session_id: &str) -> Self {
        Self {
            user: user.to_owned(),
            notice: Notice {
                title: String::new(),
                body: String::new(),
                session_id: session_id.to_owned(),
                app: None,
                approval: None,
                changed: true,
                group: false,
            },
            desktop: None,
            to: Reach::Everyone,
        }
    }

    /// The notice device `id` of `kind` gets; `None` when the notice is not for it.
    pub fn for_device(&self, id: &EndpointId, kind: &DeviceKind) -> Option<&Notice> {
        if !self.to.includes(id, kind) {
            return None;
        }
        Some(match &self.desktop {
            Some(desktop) if matches!(kind, DeviceKind::Desktop {}) => desktop,
            _ => &self.notice,
        })
    }

    /// Tells the user's devices that group `id` changed.
    pub fn group_changed(user: &str, id: &str) -> Self {
        let mut notice = Self::changed(user, id);
        notice.notice.group = true;
        notice
    }
}

pub struct Message {
    pub session_id: Uuid,
    pub request_id: Uuid,
    pub message: String,
    pub cron_id: Option<String>,
    /// A routine's label, empty when it has none. Set for routine runs only.
    pub scheduled: Option<String>,
    /// A routine whose reply always reaches the user; set for the evening digest only.
    pub always_sent: bool,
    /// An event's label when an event woke the agent (see `crate::routines::wake`).
    pub woken: Option<Woken>,
    pub images: Vec<String>,
    /// Other files sent with the message, such as a PDF.
    pub files: Vec<Upload>,
    /// The recording a held reply came as; `message` holds its transcript.
    pub voice: Option<Upload>,
    /// Said aloud in live mode; `message` is what was heard.
    pub spoken: bool,
    /// Set when the run resumes a session that stopped to ask the user.
    pub answer: Option<Answer>,
    /// A one-off model for this run, instead of the user's picked one.
    pub model: Option<Model>,
    /// A one-off thinking for this run, instead of the user's picked one.
    pub thinking: Option<ReasoningLevel>,
    /// The device the message came from. A computer has no phone to act on.
    pub origin: DeviceKind,
    /// The app the message came from, when it says.
    pub platform: Option<Platform>,
    /// What the app draws under a reply, so the run gets those tools.
    pub draws: Draws,
    /// The lead's background turn telling the user a research ended.
    pub finished_research: bool,
}

impl Message {
    /// Something the user typed, not a routine, event or card answer.
    pub(crate) fn is_prompt(&self) -> bool {
        self.cron_id.is_none()
            && self.scheduled.is_none()
            && self.woken.is_none()
            && self.answer.is_none()
            && !self.finished_research
    }

    /// A prompt said aloud, live or as a held reply.
    fn is_spoken(&self) -> bool {
        self.is_prompt() && (self.spoken || self.voice.is_some())
    }
}

/// What an app draws under a reply; a run only gets tools whose results its app shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Draws {
    /// `show_chart` and `show_stock_chart`.
    pub charts: bool,
    /// `show_cards`.
    pub cards: bool,
    /// Source chips: the app shows the web pages a reply used, so the model need not list them.
    pub sources: bool,
}

/// The user's answer to the tool call a session stopped on.
#[derive(Debug, Clone)]
pub struct Answer {
    pub call_id: String,
    pub decision: ApprovalDecision,
    /// With `Allow`, the action also runs without asking for the rest of the chat.
    pub for_session: bool,
}

impl Message {
    /// The run's request id, keyed by the call so answering twice returns the same run.
    pub(crate) fn request(&self) -> String {
        match &self.answer {
            Some(answer) => format!("approval:{}", answer.call_id),
            None => self.request_id.to_string(),
        }
    }
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

    /// The user `endpoint` is, started on first connection; `None` for a removed or refused key.
    pub async fn sign_in(self: &Arc<Self>, endpoint: &EndpointId) -> Result<Option<SignedIn>> {
        Ok(match self.admit(endpoint).await? {
            Admission::SignedIn(signed_in) => Some(signed_in),
            Admission::Revoked | Admission::NotInvited | Admission::Guest => None,
        })
    }

    /// Who `endpoint` signs in as; an unlinked key makes its own account unless single-user rules forbid.
    pub async fn admit(self: &Arc<Self>, endpoint: &EndpointId) -> Result<Admission> {
        let Some(instance) = &self.instance else {
            return Ok(match self.store.sign_in(endpoint).await? {
                SignIn::Account {
                    account,
                    device,
                    primary,
                } => Admission::SignedIn(self.signed_in(&account, device, primary)),
                SignIn::Revoked => Admission::Revoked,
            });
        };
        let sign_in = match self.store.linked_sign_in(endpoint).await? {
            Some(sign_in) => sign_in,
            None => match self.claim_new(instance, endpoint).await? {
                Some(sign_in) => sign_in,
                None => return Ok(Admission::Guest),
            },
        };
        Ok(match sign_in {
            SignIn::Account {
                account,
                device,
                primary,
            } => {
                if !self.owns(instance, &account, endpoint).await? {
                    return Ok(Admission::NotInvited);
                }
                Admission::SignedIn(self.signed_in(&account, device, primary))
            }
            SignIn::Revoked => Admission::Revoked,
        })
    }

    /// Who an already-linked key signs in as; `None` for a revoked or unlinked key.
    pub async fn sign_in_linked(
        self: &Arc<Self>,
        endpoint: &EndpointId,
    ) -> Result<Option<SignedIn>> {
        let mut sign_in = self.store.linked_sign_in(endpoint).await?;
        if let Some(instance) = &self.instance
            && sign_in.is_none()
            && self.link_local_node(instance, endpoint).await?
        {
            sign_in = self.store.linked_sign_in(endpoint).await?;
        }
        Ok(match sign_in {
            Some(SignIn::Account {
                account,
                device,
                primary,
            }) => {
                if let Some(instance) = &self.instance
                    && !self.owns(instance, &account, endpoint).await?
                {
                    return Ok(None);
                }
                Some(self.signed_in(&account, device, primary))
            }
            Some(SignIn::Revoked) | None => None,
        })
    }

    fn signed_in(
        self: &Arc<Self>,
        account: &Account,
        device: DeviceKind,
        primary: bool,
    ) -> SignedIn {
        SignedIn {
            user: self.start_user(account),
            device,
            primary,
        }
    }

    /// The single-user settings, when the server runs for one owner.
    pub fn instance(&self) -> Option<&Instance> {
        self.instance.as_ref()
    }

    /// The instance owner's account, read once and then kept.
    pub async fn instance_owner(&self) -> Result<Option<String>> {
        let Some(instance) = &self.instance else {
            return Ok(None);
        };
        if let Some(owner) = instance.owner() {
            return Ok(Some(owner));
        }
        let owner = self.store.instance_owner().await?;
        if let Some(owner) = &owner {
            instance.set_owner(owner);
        }
        Ok(owner)
    }

    /// Makes `endpoint` the owner's new account when nobody owns the instance and the config allows.
    async fn claim_new(
        &self,
        instance: &Instance,
        endpoint: &EndpointId,
    ) -> Result<Option<SignIn>> {
        if instance.owner_key().is_some_and(|key| key != endpoint)
            || self.instance_owner().await?.is_some()
        {
            log::info!("refused a key that is not the owner's; key={endpoint}");
            return Ok(None);
        }
        let claimed = self.store.claim_instance_new(endpoint).await?;
        if let Some(SignIn::Account { account, .. }) = &claimed {
            log::info!("instance claimed; user={} key={endpoint}", account.id);
            instance.set_owner(&account.id);
        }
        Ok(claimed)
    }

    /// Whether `account` owns the instance, claiming it when unowned and the config allows this key.
    async fn owns(
        &self,
        instance: &Instance,
        account: &Account,
        endpoint: &EndpointId,
    ) -> Result<bool> {
        if let Some(owner) = self.instance_owner().await? {
            return Ok(owner == account.id);
        }
        if instance.owner_key().is_some_and(|key| key != endpoint) {
            return Ok(false);
        }
        if self.store.claim_instance(&account.id, endpoint).await? {
            log::info!("instance claimed; user={} key={endpoint}", account.id);
            instance.set_owner(&account.id);
            return Ok(true);
        }
        // Another key claimed it a moment ago.
        Ok(self.instance_owner().await?.as_deref() == Some(account.id.as_str()))
    }

    /// Links `endpoint` to the owner when it is the bundled node's key; `false` otherwise.
    async fn link_local_node(
        self: &Arc<Self>,
        instance: &Instance,
        endpoint: &EndpointId,
    ) -> Result<bool> {
        let Some(node) = instance.local_node()? else {
            return Ok(false);
        };
        if node.endpoint != *endpoint {
            return Ok(false);
        }
        let Some(owner) = self.instance_owner().await? else {
            log::info!("the bundled node waits for the instance to have an owner");
            return Ok(false);
        };
        let Some(account) = self.store.account(&owner).await? else {
            return Ok(false);
        };
        let key = NewKey {
            endpoint: node.endpoint,
            name: &node.name,
            kind: DeviceKind::Desktop {},
        };
        let user = self.start_user(&account);
        match user.link_key(key).await? {
            LinkKey::Linked(_) => {
                log::info!("bundled node linked; user={owner} key={endpoint}");
                user.changed("");
                Ok(true)
            }
            LinkKey::Taken => Ok(false),
        }
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

    pub async fn submit(self: &Arc<Self>, user: &Arc<User>, message: Message) -> Result<CreateRun> {
        // Serialize registration with cancellation so a newly created run can always be stopped.
        let runs = self.runs.lock().await;
        self.submit_locked(runs, user, message).await
    }

    async fn submit_locked(
        self: &Arc<Self>,
        mut runs: tokio::sync::MutexGuard<'_, HashMap<String, Arc<LiveRun>>>,
        user: &Arc<User>,
        message: Message,
    ) -> Result<CreateRun> {
        let result = user
            .create_run(message.session_id, &message.request(), &message.message)
            .await?;
        if let CreateRun::Created(run) = &result {
            let live = Arc::new(LiveRun::new(CancellationToken::new()));
            live.push(RunEvent::RunStarted {
                run_id: run.id.clone(),
                session_id: run.session_id.clone(),
            });
            if message.voice.is_some() {
                live.push(RunEvent::Transcript {
                    text: message.message.clone(),
                });
            }
            runs.insert(run.id.clone(), live.clone());
            drop(runs);

            let (app, user, run) = (self.clone(), user.clone(), run.clone());
            let session = SessionRef {
                user: user.id().to_owned(),
                id: message.session_id,
            };
            // A woken chat is named after its event, and only if it is kept.
            match self.store.needs_first_title(&session).await {
                Ok(true) if message.woken.is_none() => {
                    let (app, text) = (self.clone(), message.message.clone());
                    tokio::spawn(async move {
                        let generator = app.generator(&session.user);
                        if titles::name_first(&app.store, generator, &session, &text).await {
                            // Nobody listening just means no phone is online.
                            let _ = app
                                .notifications
                                .send(UserNotice::changed(&session.user, ""));
                        }
                    });
                }
                Ok(_) => {}
                Err(error) => log::warn!("unable to check title: {error}; run_id={}", run.id),
            }
            tokio::spawn(async move {
                // Saved before the run can finish, which replaces it.
                if message.is_prompt()
                    && let Err(error) = user.save_prompt(&run, &message.message).await
                {
                    log::warn!("unable to save the prompt: {error:#}; run_id={}", run.id);
                }
                let turn = metering::in_turn(app.execute(&user, &run, &message, &live));
                if let Err(error) = turn.await {
                    log::error!("run failed: {error}; run_id={}", run.id);
                    if let Err(error) = user.finish(&run, None, RunStatus::Failed).await {
                        log::error!("unable to persist failure: {error}; run_id={}", run.id);
                    }
                    live.push(RunEvent::RunFinished {
                        status: RunStatus::Failed.as_str().to_owned(),
                        updated_at: None,
                    });
                }
                app.runs.lock().await.remove(&run.id);
                live.done.cancel();
                // Node reports wait for the chat to be free.
                app.subagents.turn_ended();
            });
        }
        Ok(result)
    }

    /// The event feed of a run in progress, `None` once it has finished.
    pub async fn live(&self, id: &str) -> Option<Arc<LiveRun>> {
        self.runs.lock().await.get(id).cloned()
    }

    /// Waits up to `limit` for `chat`'s run to be saved; `false` when one is still going.
    pub(crate) async fn run_ended(&self, user: &User, chat: Uuid, limit: Duration) -> Result<bool> {
        let Some(run) = user.active_run(chat).await? else {
            return Ok(true);
        };
        // Gone from the live runs means it is saved or being saved now.
        if let Some(live) = self.live(&run.id).await
            && timeout(limit, live.done.cancelled()).await.is_err()
        {
            return Ok(false);
        }
        Ok(true)
    }

    /// Stops the run and returns once it is saved, so the next message can follow at once.
    pub async fn cancel(&self, id: &str) {
        if let Some(live) = self.live(id).await {
            live.cancel.cancel();
            let _ = timeout(STOPPING_WAIT, live.done.cancelled()).await;
        }
    }

    pub async fn shutdown(&self) {
        // Read by runs as they stop, so they end as interrupted, not cancelled.
        self.shutting_down.store(true, Ordering::Relaxed);
        self.events.stop();
        self.watchers.stop();
        self.subagents.stop();
        self.stop_teams().await;
        for live in self.runs.lock().await.values() {
            live.cancel.cancel();
        }
        let _ = timeout(Duration::from_secs(10), async {
            loop {
                if self.runs.lock().await.is_empty() {
                    break;
                }
                sleep(Duration::from_millis(25)).await;
            }
        })
        .await;
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
mod tests {
    use super::*;
    use iroh::SecretKey;

    fn key(n: u8) -> EndpointId {
        SecretKey::from_bytes(&[n; 32]).public()
    }

    fn titled(title: &str) -> Notice {
        Notice {
            title: title.into(),
            ..UserNotice::changed("u1", "").notice
        }
    }

    #[test]
    fn a_notice_reaches_only_the_devices_it_names() {
        let (phone, mac, other_mac) = (key(1), key(2), key(3));
        let (is_phone, is_mac) = (DeviceKind::Phone {}, DeviceKind::Desktop {});
        let mut sent = UserNotice {
            user: "u1".into(),
            notice: titled("plain"),
            desktop: Some(titled("for computers")),
            to: Reach::Everyone,
        };
        let got = |sent: &UserNotice, id, kind| sent.for_device(id, kind).map(|n| n.title.clone());
        assert_eq!(got(&sent, &phone, &is_phone).as_deref(), Some("plain"));
        assert_eq!(got(&sent, &mac, &is_mac).as_deref(), Some("for computers"));

        sent.to = Reach::Desktops;
        assert_eq!(got(&sent, &phone, &is_phone), None, "never a phone");
        assert_eq!(got(&sent, &mac, &is_mac).as_deref(), Some("for computers"));
        assert_eq!(
            got(&sent, &other_mac, &is_mac).as_deref(),
            Some("for computers")
        );

        sent.to = Reach::Device(phone);
        assert_eq!(got(&sent, &phone, &is_phone).as_deref(), Some("plain"));
        assert_eq!(got(&sent, &mac, &is_mac), None);

        sent.to = Reach::Device(mac);
        assert_eq!(got(&sent, &phone, &is_phone), None);
        assert_eq!(got(&sent, &mac, &is_mac).as_deref(), Some("for computers"));
        assert_eq!(
            got(&sent, &other_mac, &is_mac),
            None,
            "only the Mac that started it"
        );
    }
}
