pub mod admin;
mod auto;
mod context;
mod device;
mod group;
mod group_reminders;
mod group_tools;
mod learning;
mod notify;
mod research;
mod run;
mod skills;
mod starters;
mod voice;

pub(crate) use group::group_message;

use crate::{
    AsyncMutex,
    apps::{Composio, ComposioSettings},
    compaction::{self, Summarizer},
    credentials::CredentialKey,
    decision::DecisionTool,
    images::Describer,
    instance::{Instance, InstanceConfig},
    live::LiveRun,
    metering,
    models::ModelChoices,
    protocol::{ApprovalDecision, DeviceKind, Notice, Platform, RunEvent, Upload},
    search::SearchTool,
    starters::Generator,
    store::{Account, CreateRun, LinkKey, NewKey, RunStatus, SessionRef, SignIn, Store},
    stt::{RealtimeStt, Unavailable},
    titles,
    tool_images::{ToolImages, WithToolImages},
    tools::Transitous,
    user::{Shared, User},
    voice::{Voice, VoiceConfig},
    wake::Woken,
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

/// The server's shared state: storage, users, the model and the
/// integrations every request and background task works through.
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
    /// Summarizes long sessions; see `Config::compaction_model`.
    compaction_model: Option<Model>,
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
    pub checkouts: crate::imports::Checkouts,
    /// Images tools hand the model, such as node screenshots.
    pub tool_images: Arc<ToolImages>,
    /// Speaks voice notes and transcribes spoken replies.
    voice: Voice,
    /// Transcribes speech live, for `Transcribe`.
    realtime: Arc<dyn RealtimeStt>,
    /// Live transcriptions running at once, server-wide.
    live_transcriptions: voice::LiveSlots,
    /// Account events on their way to the agent (see `crate::events`).
    pub events: crate::events::Hub,
    /// Sub-agent runs outside the chats, such as event triage.
    pub subagents: crate::subagents::Subagents,
    /// Set when the server runs for one owner (see `crate::instance`).
    instance: Option<Instance>,
    /// Agents that read finished chats over (see `crate::watchers`).
    pub watchers: crate::watchers::Hub,
    /// Seals and opens users' credentials.
    pub credential_key: CredentialKey,
}

/// Optional tools and integrations that need server credentials, and the
/// single-user instance settings, as `App::new` takes them.
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
    /// Summarizes long sessions; see `Config::compaction_model`.
    pub compaction_model: Option<Model>,
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
    /// Seals users' credentials (see `crate::credentials`). `None` makes a
    /// key that lasts as long as the process, for tests.
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
    /// A key nobody linked, on a single-user server with an owner: it may
    /// only join the owner's account with a code (see `crate::guest`).
    Guest,
}

/// A `Notice` addressed to one user's open notification streams.
#[derive(Debug, Clone)]
pub struct UserNotice {
    pub user: String,
    pub notice: Notice,
}

impl UserNotice {
    /// Tells the user's phones that `session_id` changed, or the chat list
    /// when it is empty, so a second phone shows the same chats.
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
        }
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
    /// A routine whose reply always reaches the user, as if it had called
    /// `notify_user`. Set for the evening digest only.
    pub always_sent: bool,
    /// An event's label when an event woke the agent (see `crate::wake`).
    pub woken: Option<Woken>,
    pub images: Vec<String>,
    /// Other files sent with the message, such as a PDF.
    pub files: Vec<Upload>,
    /// The recording a held reply came as. `message` holds its transcript;
    /// the recording itself is not kept.
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
    /// The lead's turn that tells the user a research ended. It runs in
    /// the background and notifies like a routine.
    pub finished_research: bool,
}

impl Message {
    /// Something the user typed, rather than a routine, an event or an
    /// answer to a card.
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

/// What an app draws under a reply beyond text and images. A run only gets
/// the tools whose results its app can show.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Draws {
    /// `show_chart` and `show_stock_chart`.
    pub charts: bool,
    /// `show_cards`.
    pub cards: bool,
    /// Source chips: the app shows the web pages a reply used under it, so
    /// the model need not list them.
    pub sources: bool,
}

/// The user's answer to the tool call a session stopped on.
#[derive(Debug, Clone)]
pub struct Answer {
    pub call_id: String,
    pub decision: ApprovalDecision,
}

impl Message {
    /// The run's request id. Answers are keyed by the call, so answering
    /// twice returns the same run.
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
            compaction_model,
            transitous,
            voice,
            realtime,
            mistral,
            instance,
            credential_key,
        } = tools;
        let (notifications, _) = broadcast::channel(64);
        let tool_images = Arc::new(ToolImages::default());
        let provider: Arc<dyn LlmProvider> =
            Arc::new(WithToolImages::new(provider, tool_images.clone()));

        let shared = Shared {
            store: store.clone(),
            notifications: notifications.clone(),
            composio: composio.map(|settings| Arc::new(Composio::new(settings))),
            decision: decision.clone(),
        };
        let voice = Voice::new(provider.clone(), mistral, voice);
        let subagents =
            crate::subagents::Subagents::new(provider.clone(), llm.clone(), notifications.clone());
        Arc::new(Self {
            store,
            notifications,
            provider,
            llm,
            vision_model,
            compaction_model,
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
            checkouts: crate::imports::Checkouts::default(),
            tool_images,
            voice,
            realtime: realtime.unwrap_or_else(|| Arc::new(Unavailable)),
            live_transcriptions: voice::LiveSlots::default(),
            events: crate::events::Hub::default(),
            subagents,
            instance: instance.map(Instance::new),
            watchers: crate::watchers::Hub::default(),
            credential_key: credential_key.unwrap_or_else(CredentialKey::random),
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

    /// The user `endpoint` is, made and started on its first connection.
    /// `None` for a key that was linked and then removed, or one a
    /// single-user server refuses.
    pub async fn sign_in(self: &Arc<Self>, endpoint: &EndpointId) -> Result<Option<SignedIn>> {
        Ok(match self.admit(endpoint).await? {
            Admission::SignedIn(signed_in) => Some(signed_in),
            Admission::Revoked | Admission::NotInvited | Admission::Guest => None,
        })
    }

    /// Who `endpoint` signs in as. A key nobody linked makes its own
    /// account, except on a single-user server, where only the first one
    /// may, and only when the config names no other owner.
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

    /// Who a key that must already be linked signs in as: `None` for a
    /// revoked key or one nobody linked, which `sign_in` would give an
    /// account of its own. On a single-user server the bundled node is
    /// linked to the owner here, and only the owner's keys sign in.
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

    /// Makes `endpoint`, which no account has, the owner's new account when
    /// nobody owns the instance yet and the config names no other key.
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

    /// Whether `account`, which `endpoint` signs into, owns the instance. An
    /// account made before the server ran for one owner claims it when
    /// nobody has, if the config names no owner or names this key.
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

    /// Links `endpoint` to the owner when it is the bundled node's key.
    /// False when it is not, or nobody owns the instance yet.
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

    /// Starts every user that signed in before, so their crons and feeds
    /// run without a connection.
    pub async fn start_users(self: &Arc<Self>) -> Result<()> {
        for account in self.store.accounts().await? {
            self.start_user(&account);
        }
        Ok(())
    }

    /// After `source` merged into `target`: stops the gone account and
    /// starts the target again, so it picks up an outside id it took over.
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

    /// Waits up to `limit` for the run going on in `user`'s `chat` to be
    /// saved. `true` once the chat has none, `false` when it still has one.
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

    /// Stops the run and returns once it is saved, so the app that stopped it
    /// can send the chat's next message right after.
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

    /// How long the model reasons on `user`'s turns: their pick, else the
    /// config's.
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

    /// The model that describes images: the configured vision model, or
    /// `own`, the assistant's, when it sees images.
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
        // The limit follows the model the sessions run on, not the summarizer.
        let window = self.provider.context_window(&self.llm.model).await;
        compaction::compact_idle(
            &self.store,
            self.summarizer(),
            compaction::compact_at(window),
        )
        .await;
    }

    /// Compacts `session` in the background when the last prompt of its turn
    /// on `model` reached the limit, so the next message starts small.
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
            compaction::compact_session(&app.store, &app.summarizer(), &session, limit).await;
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

    /// Summarizes sessions on the compaction model, else the assistant's.
    fn summarizer(&self) -> Summarizer<'_> {
        Summarizer {
            provider: self.provider.as_ref(),
            model: self.compaction_model.as_ref().unwrap_or(&self.llm.model),
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
