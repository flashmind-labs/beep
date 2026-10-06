mod context;
mod device;
mod learning;
mod notify;
mod run;
mod starters;

use crate::{
    apps::{Composio, ComposioSettings},
    compaction::{self, Summarizer},
    decision::DecisionTool,
    images::Describer,
    live::LiveRun,
    models::ModelChoices,
    protocol::{ApprovalDecision, DeviceAnswer, ForegroundApp, Notice, RunEvent},
    sandbox::Sandbox,
    search::SearchTool,
    starters::Generator,
    store::{CreateRun, RunStatus, SessionRef, Store},
    titles,
    user::{Shared, User},
};
use anyhow::Result;
use flashmind::{
    core::CancellationToken,
    types::{AgentLlmConfig, LlmProvider, Model, ReasoningLevel},
};
use iroh::EndpointId;
use secrecy::SecretString;
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex as StdMutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex, broadcast, oneshot};
use uuid::Uuid;

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
    /// What admins may run their own turns on.
    pub models: ModelChoices,
    /// What a new `User` is built from.
    shared: Shared,
    /// Every user signed in since the server started, by id.
    users: StdMutex<HashMap<String, Arc<User>>>,
    /// Runs in progress, with their event feed.
    runs: Mutex<HashMap<String, Arc<LiveRun>>>,
    /// Questions sent to a phone, keyed by query id, waiting for its answer.
    device_queries: Mutex<HashMap<String, oneshot::Sender<DeviceAnswer>>>,
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
    /// Per-user files and code execution; see `Config::sandbox_dir`.
    pub sandbox: Option<Arc<Sandbox>>,
}

/// The part of `ServerTools` a run registers itself.
struct RunTools {
    search: Option<Arc<SearchTool>>,
    firecrawl_key: Option<SecretString>,
    decision: Option<Arc<DecisionTool>>,
    sandbox: Option<Arc<Sandbox>>,
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
            },
        }
    }
}

#[derive(Deserialize)]
pub struct Message {
    pub session_id: Uuid,
    pub request_id: Uuid,
    pub message: String,
    #[serde(default)]
    pub cron_id: Option<String>,
    /// A routine's label, empty when it has none. Set for routine runs only.
    #[serde(skip)]
    pub scheduled: Option<String>,
    #[serde(default)]
    pub images: Vec<String>,
    #[serde(default)]
    pub foreground: Option<ForegroundApp>,
    /// Set when the run resumes a session that stopped to ask the user.
    #[serde(skip)]
    pub answer: Option<Answer>,
    /// An admin's one-off model for this run, instead of their picked one.
    #[serde(skip)]
    pub model: Option<Model>,
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
            sandbox,
        } = tools;
        let (notifications, _) = broadcast::channel(64);
        let shared = Shared {
            store: store.clone(),
            notifications: notifications.clone(),
            composio: composio.map(|settings| Arc::new(Composio::new(settings))),
        };
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
                sandbox,
            },
            shared,
            users: StdMutex::new(HashMap::new()),
            runs: Mutex::new(HashMap::new()),
            device_queries: Mutex::new(HashMap::new()),
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

    /// The user `endpoint` is, made and started on its first connection.
    pub async fn sign_in(self: &Arc<Self>, endpoint: &EndpointId) -> Result<Arc<User>> {
        let id = self.store.sign_in(endpoint).await?;
        Ok(self.start_user(&id))
    }

    /// Starts every user that signed in before, so their crons and feeds
    /// run without a connection.
    pub async fn start_users(self: &Arc<Self>) -> Result<()> {
        for id in self.store.user_ids().await? {
            self.start_user(&id);
        }
        Ok(())
    }

    fn start_user(self: &Arc<Self>, id: &str) -> Arc<User> {
        let mut users = self.users.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(user) = users.get(id) {
            return user.clone();
        }
        let user = User::new(id, &self.shared);
        users.insert(id.to_owned(), user.clone());
        // `start` only spawns, so holding the lock here is fine.
        user.start(self);
        log::info!("user started; user={id}");
        user
    }

    pub async fn submit(self: &Arc<Self>, user: &Arc<User>, message: Message) -> Result<CreateRun> {
        // Serialize registration with cancellation so a newly created run can always be stopped.
        let mut runs = self.runs.lock().await;
        let result = user
            .create_run(
                &message.session_id.to_string(),
                &message.request(),
                &message.message,
            )
            .await?;
        if let CreateRun::Created(run) = &result {
            let live = Arc::new(LiveRun::new(CancellationToken::new()));
            live.push(RunEvent::RunStarted {
                run_id: run.id.clone(),
                session_id: run.session_id.clone(),
            });
            runs.insert(run.id.clone(), live.clone());

            let (app, user, run) = (self.clone(), user.clone(), run.clone());
            let session = SessionRef {
                user: user.id().to_owned(),
                id: run.session_id.clone(),
            };
            match self.store.needs_first_title(&session).await {
                Ok(true) => {
                    let (app, text) = (self.clone(), message.message.clone());
                    tokio::spawn(async move {
                        let generator = Generator {
                            provider: app.provider.as_ref(),
                            llm: &app.llm,
                            user: &session.user,
                        };
                        if titles::name_first(&app.store, generator, &session, &text).await {
                            // Nobody listening just means no phone is online.
                            let _ = app
                                .notifications
                                .send(UserNotice::changed(&session.user, ""));
                        }
                    });
                }
                Ok(false) => {}
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
            });
        }
        Ok(result)
    }

    /// The event feed of a run in progress, `None` once it has finished.
    pub async fn live(&self, id: &str) -> Option<Arc<LiveRun>> {
        self.runs.lock().await.get(id).cloned()
    }

    pub async fn cancel(&self, id: &str) {
        if let Some(live) = self.live(id).await {
            live.cancel.cancel();
        }
    }

    pub async fn shutdown(&self) {
        // Read by runs as they stop, so they end as interrupted, not cancelled.
        self.shutting_down.store(true, Ordering::Relaxed);
        for live in self.runs.lock().await.values() {
            live.cancel.cancel();
        }
        let _ = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if self.runs.lock().await.is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
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
        let summarizer = Summarizer {
            provider: self.provider.as_ref(),
            model: &self.llm.model,
        };
        compaction::compact_idle(&self.store, summarizer).await;
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

    /// Plain completions on the assistant's model for `user`.
    pub fn generator<'a>(&'a self, user: &'a str) -> Generator<'a> {
        Generator {
            provider: self.provider.as_ref(),
            llm: &self.llm,
            user,
        }
    }
}
