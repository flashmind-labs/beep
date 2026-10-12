pub mod admin;
mod auto;
mod context;
mod device;
mod group;
mod group_reminders;
mod group_tools;
mod learning;
mod notify;
mod run;
mod skills;
mod starters;

pub(crate) use group::group_message;

use crate::{
    AsyncMutex,
    agents::AgentId,
    apps::{Composio, ComposioSettings},
    compaction::{self, Summarizer},
    decision::DecisionTool,
    images::Describer,
    live::LiveRun,
    models::ModelChoices,
    protocol::{ApprovalDecision, DeviceKind, Notice, Platform, RunEvent},
    sandbox::Sandbox,
    search::SearchTool,
    starters::Generator,
    store::{Account, CreateRun, RunStatus, SessionRef, SignIn, Store},
    titles,
    tools::Transitous,
    user::{Shared, User},
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
    /// What admins may run their own turns on.
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
}

/// Optional tools and integrations that need server credentials, as
/// `App::new` takes them.
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
    /// Per-user files and code execution; see `Config::sandbox_dir`.
    pub sandbox: Option<Arc<Sandbox>>,
    /// Transit routes for `get_route`.
    pub transitous: Option<Arc<Transitous>>,
}

/// The part of `ServerTools` a run registers itself.
struct RunTools {
    search: Option<Arc<SearchTool>>,
    firecrawl_key: Option<SecretString>,
    decision: Option<Arc<DecisionTool>>,
    sandbox: Option<Arc<Sandbox>>,
    transitous: Option<Arc<Transitous>>,
}

/// Who a connection signed in as.
pub struct SignedIn {
    pub user: Arc<User>,
    pub device: DeviceKind,
    /// The key that made the account, which may link and remove others.
    pub primary: bool,
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
    /// The agent a routine runs as: the one that set it up.
    pub agent: Option<AgentId>,
    /// A routine's label, empty when it has none. Set for routine runs only.
    pub scheduled: Option<String>,
    /// An event's label when an event woke the agent (see `crate::wake`).
    pub woken: Option<String>,
    pub images: Vec<String>,
    /// Set when the run resumes a session that stopped to ask the user.
    pub answer: Option<Answer>,
    /// An admin's one-off model for this run, instead of their picked one.
    pub model: Option<Model>,
    /// An admin's one-off thinking for this run, instead of their picked one.
    pub thinking: Option<ReasoningLevel>,
    /// The device the message came from. A computer has no phone to act on.
    pub origin: DeviceKind,
    /// The app the message came from, when it says.
    pub platform: Option<Platform>,
    /// Whether the app draws charts, so the run gets the chart tools.
    pub charts: bool,
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
            sandbox,
            transitous,
        } = tools;
        let (notifications, _) = broadcast::channel(64);

        let shared = Shared {
            store: store.clone(),
            notifications: notifications.clone(),
            composio: composio.map(|settings| Arc::new(Composio::new(settings))),
            decision: decision.clone(),
        };
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
                sandbox,
                transitous,
            },
            shared,
            users: Mutex::new(HashMap::new()),
            runs: AsyncMutex::new(HashMap::new()),
            device_queries: device::DeviceQueries::default(),
            groups_busy: Mutex::new(HashSet::new()),
            group_actions: Mutex::new(HashMap::new()),
            shutting_down: AtomicBool::new(false),
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
    /// `None` for a key that was linked and then removed.
    pub async fn sign_in(self: &Arc<Self>, endpoint: &EndpointId) -> Result<Option<SignedIn>> {
        Ok(match self.store.sign_in(endpoint).await? {
            SignIn::Account {
                account,
                device,
                primary,
            } => {
                self.models.admit(&account.id, &endpoint.to_string());
                Some(SignedIn {
                    user: self.start_user(&account),
                    device,
                    primary,
                })
            }
            SignIn::Revoked => None,
        })
    }

    /// Starts every user that signed in before, so their crons and feeds
    /// run without a connection.
    pub async fn start_users(self: &Arc<Self>) -> Result<()> {
        for account in self.store.accounts().await? {
            for key in self.store.live_keys(&account.id).await? {
                self.models.admit(&account.id, &key);
            }
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
        for key in self.store.live_keys(target).await? {
            self.models.admit(target, &key);
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
        let mut runs = self.runs.lock().await;
        let result = user
            .create_run(message.session_id, &message.request(), &message.message)
            .await?;
        if let CreateRun::Created(run) = &result {
            let live = Arc::new(LiveRun::new(CancellationToken::new()));
            live.push(RunEvent::RunStarted {
                run_id: run.id.clone(),
                session_id: run.session_id.clone(),
            });
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
                if let Err(error) = app.execute(&user, &run, &message, &live).await {
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
            });
        }
        Ok(result)
    }

    /// The event feed of a run in progress, `None` once it has finished.
    pub async fn live(&self, id: &str) -> Option<Arc<LiveRun>> {
        self.runs.lock().await.get(id).cloned()
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
        self.models.for_user(user.id(), picked.as_deref()).clone()
    }

    /// How long the model reasons on `user`'s turns: their pick when they are
    /// an admin, else the config's.
    pub(crate) async fn thinking_for(&self, user: &User) -> ReasoningLevel {
        if !self.models.is_admin(user.id()) {
            return self.llm.reasoning;
        }
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

    /// Plain completions on the assistant's model for `user`.
    pub fn generator<'a>(&'a self, user: &'a str) -> Generator<'a> {
        Generator {
            provider: self.provider.as_ref(),
            llm: &self.llm,
            user,
        }
    }
}
