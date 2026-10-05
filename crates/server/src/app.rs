use crate::{
    apps::{Composio, ComposioSettings},
    compaction::{self, Summarizer},
    decision::DecisionTool,
    images::{self, Describer},
    live::LiveRun,
    memories::{self, Exchange, Hit, Turn},
    models::ModelChoices,
    permissions::{self, ApprovalPayload, Approved, Rules},
    protocol::{
        AppIntegration, Approval, ApprovalDecision, DeviceAnswer, DeviceQuery, ForegroundApp,
        Notice, PendingApproval, PromptSuggestions, Run, RunEvent, Usage,
    },
    repeat::{self, RepeatGuard},
    sandbox::{self, Sandbox},
    search::SearchTool,
    starters::{Generator, LocalTime, MemoryNote, StarterContext},
    store::{CreateRun, FinishedTurn, Location, RunStatus, SessionRef, Store},
    timing::{Output, TurnTimer},
    titles, tools,
    user::{Shared, User},
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use flashmind::{
    core::{Agent, CancellationToken, Conversation, ConversationEntry},
    tools::{
        firecrawl::{WebCrawlTool, WebExtractTool, WebMapTool, WebScrapeTool},
        search_cache::{SearchCacheRef, SearchResultCache},
        search_read::WebSearchReadTool,
    },
    types::{
        AgentEvent, AgentInput, AgentLlmConfig, ContentPart, LlmProvider, Model, ToolCall,
        ToolRegistry, tool::ToolResult,
    },
};
use futures::StreamExt;
use iroh::EndpointId;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use std::{
    collections::HashMap,
    fmt::Write,
    sync::{
        Arc, Mutex as StdMutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::time::Instant;

/// How long a run may go without a tool call succeeding. Text, reasoning and
/// failed calls are not progress; time spent waiting on a tool does not count.
/// A run that keeps getting results keeps going, up to [`MAX_RUN`].
const PROGRESS_TIMEOUT: Duration = Duration::from_secs(30);
/// The longest any run may take, progress or not.
const MAX_RUN: Duration = Duration::from_secs(15 * 60);
/// How long a run may go without a single event from the model or a tool.
const IDLE_TIMEOUT: Duration = Duration::from_secs(90);
/// The same while a tool runs: crawls and extractions poll quietly for up to
/// two minutes. The run's overall deadline still applies.
const TOOL_IDLE_TIMEOUT: Duration = Duration::from_secs(150);
/// Above this Jev score the learner reads the turn; below it nothing is stored.
const MEMORY_THRESHOLD: f64 = 0.8;
const MEMORY_QUESTION: &str = "Does this exchange reveal a lasting fact about the user worth remembering for future chats that is not already remembered, correct one that is, or ask to forget one? Such facts are their name, preferences, people, places, work, routines or plans, whether the user said it or the assistant found it in their data at their request. Answer no for small talk, one-off questions, facts only about the world, or facts the already remembered list covers.";
use tokio::sync::{Mutex, RwLock, broadcast, oneshot};
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

/// A guarded call the run stopped on.
struct Parked {
    call_id: String,
    approval: Approval,
}

impl Parked {
    fn from(call_id: &str, result: &ToolResult) -> Option<Self> {
        let payload = result.payload()?;
        let payload = payload.as_any().downcast_ref::<ApprovalPayload>()?;
        Some(Self {
            call_id: call_id.to_owned(),
            approval: payload.approval.clone(),
        })
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

    /// Send a query to the connected device and wait for its response.
    ///
    /// Emits a `device_query` event on the run's event stream so the client
    /// picks it up, then blocks until the device POSTs the result back (or the
    /// timeout expires).
    pub async fn device_query(
        &self,
        run_id: &str,
        query: DeviceQuery,
        timeout_secs: u64,
    ) -> Result<DeviceAnswer> {
        let Some(live) = self.live(run_id).await else {
            anyhow::bail!("run {run_id} is not running");
        };
        let (query_id, rx) = self.ask_device(&live, query).await;
        self.wait_device(query_id, rx, timeout_secs).await
    }

    /// Asks the user about the call the run stopped on: the chat shows a
    /// card and the phone a notification. Nothing waits; the answer starts a
    /// new run that resumes the session.
    fn park(&self, user: &User, run: &Run, live: &LiveRun, parked: Parked) {
        let Parked { call_id, approval } = parked;
        live.push(RunEvent::DeviceQuery {
            query_id: call_id.clone(),
            query: DeviceQuery::Approve {
                approval: approval.clone(),
            },
        });
        let body = approval
            .fields
            .iter()
            .map(|field| format!("{}: {}", field.label, field.value))
            .collect::<Vec<_>>()
            .join("\n");
        let _ = self.notifications.send(UserNotice {
            user: user.id().to_owned(),
            notice: Notice {
                title: format!("Allow {}: {}?", approval.app, approval.action),
                body,
                session_id: run.session_id.clone(),
                app: None,
                approval: Some(PendingApproval {
                    run_id: run.id.clone(),
                    query_id: call_id,
                    approval,
                }),
                changed: false,
            },
        });
    }

    /// Puts `query` on the run's event stream for the phone to answer.
    async fn ask_device(
        &self,
        live: &LiveRun,
        query: DeviceQuery,
    ) -> (String, oneshot::Receiver<DeviceAnswer>) {
        let query_id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.device_queries
            .lock()
            .await
            .insert(query_id.clone(), tx);
        live.push(RunEvent::DeviceQuery {
            query_id: query_id.clone(),
            query,
        });
        (query_id, rx)
    }

    async fn wait_device(
        &self,
        query_id: String,
        rx: oneshot::Receiver<DeviceAnswer>,
        timeout_secs: u64,
    ) -> Result<DeviceAnswer> {
        let result = tokio::time::timeout(Duration::from_secs(timeout_secs), rx).await;
        // Clean up on timeout or channel close.
        self.device_queries.lock().await.remove(&query_id);
        match result {
            Ok(Ok(answer)) => Ok(answer),
            Ok(Err(_)) => anyhow::bail!("device query channel closed without a response"),
            Err(_) => anyhow::bail!("device query timed out after {timeout_secs}s"),
        }
    }

    /// Called by the device-result endpoint to deliver a response for a pending
    /// device query. Returns `true` if the query_id was found and resolved.
    pub async fn resolve_device_query(&self, query_id: &str, answer: DeviceAnswer) -> bool {
        if let Some(tx) = self.device_queries.lock().await.remove(query_id) {
            tx.send(answer).is_ok()
        } else {
            false
        }
    }

    /// Tells the phone a scheduled run finished. A completed run shows its reply,
    /// which the prompt asks to be written as the reminder itself.
    pub async fn notify_run_complete(
        &self,
        user: &User,
        run: &Run,
        status: RunStatus,
        reply: &str,
    ) {
        // Build a title from the session's first message (truncated).
        let title = match user.sessions().await {
            Ok(sessions) => sessions
                .into_iter()
                .find(|s| s.id == run.session_id)
                .map(|s| s.title)
                .unwrap_or_else(|| "Beep".to_owned()),
            Err(_) => "Beep".to_owned(),
        };
        let reply = notification_text(reply);
        let body = match status {
            RunStatus::Completed if !reply.is_empty() => reply.as_str(),
            RunStatus::Completed => "Your request completed successfully.",
            RunStatus::Cancelled => "The run was cancelled.",
            RunStatus::Interrupted => "The server restarted before the run finished.",
            RunStatus::TimedOut => "The run timed out.",
            RunStatus::Stalled => "The model stopped responding.",
            RunStatus::Looped => "The model got stuck repeating itself and was stopped.",
            RunStatus::Failed => "The run failed.",
            RunStatus::Waiting => "Waiting for your OK.",
        }
        .to_owned();
        let notice = UserNotice {
            user: user.id().to_owned(),
            notice: Notice {
                title,
                body,
                session_id: run.session_id.clone(),
                app: None,
                approval: None,
                changed: false,
            },
        };
        // Best-effort: ignore send errors (no active receivers).
        let _ = self.notifications.send(notice);
        if let Ok(tokens) = user.device_tokens().await
            && !tokens.is_empty()
        {
            log::info!(
                "would send push; user={user} run_id={} status={} devices={}",
                run.id,
                status.as_str(),
                tokens.len()
            );
        }
    }

    /// Time, location, the app behind the bubble and which apps are connected
    /// for this turn, sent as a developer message. Memories are not included:
    /// the model looks them up with `memory_recall`.
    async fn build_context(
        &self,
        user: &User,
        foreground: Option<&ForegroundApp>,
        apps: &[AppIntegration],
    ) -> Option<String> {
        let mut ctx = String::new();

        // Time with user's timezone
        if let Ok(tz_name) = user.get_timezone().await {
            let now = if let Ok(tz) = tz_name.parse::<Tz>() {
                Utc::now()
                    .with_timezone(&tz)
                    .format("%Y-%m-%d %H:%M %Z")
                    .to_string()
            } else {
                Utc::now().format("%Y-%m-%d %H:%M UTC").to_string()
            };
            let _ = writeln!(ctx, "Time: {now}");
        }

        if let Ok(Some(loc)) = user.get_location().await {
            let _ = writeln!(ctx, "{}", location_line(&loc, Utc::now()));
        }

        if let Some(app) = foreground {
            let _ = writeln!(ctx, "{}", foreground_line(app));
        }

        if let Some(line) = apps_line(apps) {
            let _ = writeln!(ctx, "{line}");
        }

        (!ctx.is_empty()).then_some(ctx)
    }

    /// The model `user`'s turns run on.
    pub(crate) async fn model_for(&self, user: &User) -> Model {
        let picked = user.model().await.unwrap_or_else(|e| {
            log::warn!("reading the picked model failed; user={}: {e:#}", user.id());
            None
        });
        self.models.for_user(user.id(), picked.as_deref()).clone()
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

    /// The model that reads screenshots for `read_screen`.
    pub(crate) async fn screen_reader(&self, user: &User) -> Option<Describer> {
        let model = self.model_for(user).await;
        let sees_images = self.provider.capabilities(&model).await.images;
        self.describer(user, &model, sees_images)
    }

    async fn execute(
        self: &Arc<Self>,
        user: &Arc<User>,
        run: &Run,
        message: &Message,
        live: &LiveRun,
    ) -> Result<()> {
        let cancel = &live.cancel;
        let is_cron = message.cron_id.is_some();
        let mut timer = TurnTimer::start();

        // The model runs on the context, which may be compacted; the transcript
        // keeps every message for the phone and gets this turn's entries appended.
        let mut transcript = user
            .conversation(&run.session_id)
            .await?
            .unwrap_or_else(Conversation::new);
        let mut conversation = user
            .context(&run.session_id)
            .await?
            .map_or_else(|| transcript.clone(), |context| context.conversation);

        // Always set the server-owned prompt, including when loading an older conversation.
        // It stays static so the provider can cache it.
        conversation.set_system(include_str!("../prompts/assistant.md"));
        transcript.set_system(include_str!("../prompts/assistant.md"));
        let model = match message.model.clone() {
            Some(model) => model,
            None => self.model_for(user).await,
        };
        let mut llm = self.llm.with_model(model.clone()).with_user(user.id());
        if let Some(routing) = self.models.routing(&model) {
            llm.provider_preferences = Some(routing.clone());
        }

        let timezone = user.get_timezone().await.ok();
        let mut registry = tools::registry(self.clone(), run, user, timezone.as_deref());
        if let Some(search) = &self.tools.search {
            registry.register(search.clone());
        }
        if let Some(key) = &self.tools.firecrawl_key {
            register_web_tools(&mut registry, key.expose_secret());
        }
        if let Some(decision) = &self.tools.decision {
            registry.register(decision.clone());
        }
        if let Some(sandbox) = &self.tools.sandbox {
            sandbox::register(&mut registry, sandbox, user);
        }

        // Calls of the last model turn still without a result: the session
        // stopped on the first one to ask the user.
        let open = permissions::open_calls(&conversation);
        let answer = message.answer.as_ref();
        if let Some(answer) = answer
            && answer.decision == (ApprovalDecision::Always {})
            && let Some(call) = open.iter().find(|call| call.id == answer.call_id)
            && let Err(error) = permissions::allow_always(user, &call.name).await
        {
            log::warn!(
                "permissions: unable to save; run_id={} error={error:#}",
                run.id
            );
        }
        let approved = Approved(
            answer
                .filter(|answer| answer.decision != (ApprovalDecision::Deny {}))
                .map(|answer| answer.call_id.clone()),
        );

        let (composio, mcp, apps) = tokio::join!(user.app_tools(), user.mcp_tools(), user.apps());
        let rules = user.app_rules().await.unwrap_or_else(|error| {
            log::warn!("permissions: unable to load, asking for everything: {error:#}");
            Rules::default()
        });
        for tool in permissions::guard(composio.into_iter().chain(mcp).collect(), rules, approved) {
            registry.register(tool);
        }
        let refusals = repeat::guard_tool_calls(&mut registry);

        // Entries from here on belong to this turn. Matching by time rather than
        // index survives the agent compacting mid-run.
        let turn_started = Utc::now();
        // Conversation entries do not record whether a tool failed; history needs it.
        let mut failed_tools = Vec::new();
        // The guarded call the run stops on to ask the user, if any.
        let mut parked = None;

        // Close the open calls before the model goes on. An answer runs them
        // in order, as if they had just been made; a new message leaves them
        // undone.
        for call in open {
            let result = match answer {
                None => ToolResult::success(&call.id, permissions::SKIPPED),
                Some(answer)
                    if answer.call_id == call.id
                        && answer.decision == (ApprovalDecision::Deny {}) =>
                {
                    ToolResult::success(&call.id, permissions::DENIED)
                }
                Some(_) => {
                    live.push(RunEvent::ToolStarted {
                        name: call.name.clone(),
                        call_id: call.id.clone(),
                        description: registry.humanize(&call),
                    });
                    registry.execute(&call, None, cancel).await
                }
            };
            if let Some(stop) = Parked::from(&call.id, &result) {
                parked = Some(stop);
                break;
            }
            conversation.add(ConversationEntry::tool(&call.id, result.output()));
            if answer.is_some() {
                if !result.is_success() {
                    failed_tools.push(call.id.clone());
                }
                live.push(RunEvent::ToolResult {
                    name: call.name,
                    call_id: call.id,
                    output: result.output(),
                    success: result.is_success(),
                });
            }
        }

        let building = Instant::now();
        let context = self
            .build_context(user, message.foreground.as_ref(), &apps.unwrap_or_default())
            .await;
        timer.context_built(building.elapsed());

        let mut agent = Agent::builder(self.provider.clone())
            .llm(llm)
            .tools(registry)
            .build()
            .await;

        // Every image is described alongside the run. A model that sees images
        // gets them now and the descriptions replace them once the turn is
        // saved; a model that does not waits for the descriptions instead.
        let attached: Vec<ContentPart> = message
            .images
            .iter()
            .filter_map(|uri| images::parse_data_uri(uri))
            .collect();
        let sees_images = !attached.is_empty() && self.provider.capabilities(&model).await.images;
        let mut describing = (!attached.is_empty())
            .then(|| self.describer(user, &model, sees_images))
            .flatten()
            .map(|describer| {
                let attached = attached.clone();
                let message = message.message.clone();
                tokio::spawn(async move { describer.describe_all(&attached, &message).await })
            });
        let parts = if sees_images {
            images::labelled(&attached)
        } else {
            let mut descriptions = match describing.take() {
                Some(task) => task.await.unwrap_or_default(),
                None => Vec::new(),
            };
            descriptions.resize(attached.len(), None);
            images::described(&descriptions)
        };

        // Flashmind saves `context` as a developer message before the user message. It
        // stays in history, which keeps the cached prefix intact and timestamps each turn.
        // An answer adds no message: the model carries on from the call results.
        let input = match answer {
            Some(_) => AgentInput::Resume,
            None => AgentInput::User {
                content: message.message.clone(),
                context,
                parts: (!parts.is_empty()).then_some(parts),
            },
        };

        // The reply the memory extractor reads once the run completes.
        let mut reply = String::new();
        // Prompt size of the last model call, which decides later compaction.
        let mut prompt_tokens = 0;
        let mut status = if parked.is_some() {
            RunStatus::Waiting
        } else {
            timer.prepared();
            let stream = agent.start(&mut conversation, cancel.clone(), input, None);
            tokio::pin!(stream);

            let deadline = tokio::time::sleep(PROGRESS_TIMEOUT);
            tokio::pin!(deadline);
            let hard_stop = tokio::time::sleep(MAX_RUN);
            tokio::pin!(hard_stop);

            let mut tools_running = 0usize;
            // A model repeating itself never stops on its own; end the run instead
            // of streaming the same sentence until the deadline.
            let mut text_guard = RepeatGuard::default();
            let mut reasoning_guard = RepeatGuard::default();
            loop {
                let event = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => break if self.shutting_down.load(Ordering::Relaxed) {
                        RunStatus::Interrupted
                    } else {
                        RunStatus::Cancelled
                    },
                    _ = &mut deadline, if tools_running == 0 => { cancel.cancel(); break RunStatus::TimedOut; },
                    _ = &mut hard_stop => { cancel.cancel(); break RunStatus::TimedOut; },
                    event = tokio::time::timeout(
                        if tools_running > 0 { TOOL_IDLE_TIMEOUT } else { IDLE_TIMEOUT },
                        stream.next(),
                    ) => match event {
                        Ok(event) => event,
                        // Nothing from the model or a tool for this long: the stream is stuck.
                        Err(_) => { cancel.cancel(); break RunStatus::Stalled; }
                    },
                };

                let data = match event {
                    Some(AgentEvent::TextDelta(delta)) => {
                        if text_guard.push(&delta) {
                            cancel.cancel();
                            break RunStatus::Looped;
                        }
                        timer.output(Output::Text);
                        reply.push_str(&delta);
                        RunEvent::TextDelta { delta }
                    }
                    Some(AgentEvent::ReasoningDelta(delta)) => {
                        if reasoning_guard.push(&delta) {
                            cancel.cancel();
                            break RunStatus::Looped;
                        }
                        timer.output(Output::Reasoning);
                        RunEvent::ReasoningDelta { delta }
                    }
                    Some(AgentEvent::ToolStart {
                        name,
                        id,
                        humanized,
                    }) => {
                        timer.output(Output::Tool);
                        tools_running += 1;
                        RunEvent::ToolStarted {
                            name,
                            call_id: id,
                            description: humanized,
                        }
                    }
                    Some(AgentEvent::ToolResult {
                        name,
                        id,
                        output,
                        success,
                        ..
                    }) => {
                        tools_running = tools_running.saturating_sub(1);
                        if tools_running == 0 {
                            timer.tools_done();
                        }
                        if success {
                            deadline.as_mut().reset(Instant::now() + PROGRESS_TIMEOUT);
                        } else {
                            failed_tools.push(id.clone());
                        }
                        RunEvent::ToolResult {
                            name,
                            call_id: id,
                            output,
                            success,
                        }
                    }
                    Some(AgentEvent::Usage(u)) => {
                        timer.usage(&u);
                        prompt_tokens = u.prompt_tokens;
                        RunEvent::Usage {
                            usage: Usage {
                                prompt_tokens: u.prompt_tokens,
                                completion_tokens: u.completion_tokens,
                                total_tokens: u.total_tokens,
                                cache_read_tokens: u.cache_read_tokens,
                                cache_creation_tokens: u.cache_creation_tokens,
                            },
                        }
                    }
                    Some(AgentEvent::Interrupted {
                        tool_call_id,
                        payload,
                        ..
                    }) => {
                        tools_running = tools_running.saturating_sub(1);
                        parked = payload
                            .as_ref()
                            .and_then(|payload| payload.as_any().downcast_ref::<ApprovalPayload>())
                            .map(|payload| Parked {
                                call_id: tool_call_id,
                                approval: payload.approval.clone(),
                            });
                        continue;
                    }
                    Some(AgentEvent::Done(_)) => break RunStatus::Completed,
                    Some(AgentEvent::Error(_)) => {
                        // Provider errors can contain upstream response details. Keep them off the wire.
                        let error = RunEvent::Error {
                            message:
                                "The model request failed. Check server provider configuration."
                                    .to_owned(),
                        };
                        live.push(error);
                        break RunStatus::Failed;
                    }
                    None => break RunStatus::Failed,
                    _ => continue,
                };

                live.push(data);
                // The model keeps repeating calls it was told will not work.
                if refusals.exhausted() {
                    cancel.cancel();
                    break RunStatus::Looped;
                }
            }
        };

        timer.streamed();
        if parked.is_some() && status == RunStatus::Completed {
            status = RunStatus::Waiting;
        }

        // A run that stops to ask notifies with the question instead.
        if is_cron && status != RunStatus::Waiting {
            // Notify registered devices for cron-triggered runs only.
            self.notify_run_complete(user, run, status, &reply).await;
        }

        // Learn from the turn while the run is still open, so what was saved shows
        // as memory tool chips under the reply. Scheduled prompts are written by
        // the user once, not said in conversation.
        if status == RunStatus::Completed && !is_cron {
            // A resumed turn learns from the message that started it.
            let asked = answer.and_then(|_| {
                conversation
                    .entries()
                    .iter()
                    .rev()
                    .find(|entry| entry.timestamp < turn_started && entry.is_user())
            });
            let turn_from = asked.map_or(turn_started, |entry| entry.timestamp);
            let generator = Generator {
                provider: self.provider.as_ref(),
                llm: &self.llm,
                user: user.id(),
            };
            let exchange = Exchange {
                session_id: run.session_id.clone(),
                today: user.today().await,
                earlier: earlier_turns(&conversation, turn_from),
                latest: Turn {
                    message: asked.map_or_else(
                        || message.message.clone(),
                        |entry| entry.content().to_owned(),
                    ),
                    reply,
                },
            };
            let known = user
                .related_memories(&exchange)
                .await
                .unwrap_or_else(|error| {
                    log::warn!(
                        "unable to find related memories: {error}; run_id={}",
                        run.id
                    );
                    Vec::new()
                });
            let score = self.memory_score(memory_state(&known, &exchange)).await;
            if let Some(probability) = score {
                live.push(RunEvent::MemoryScore { probability });
                // Kept by the time of the turn's user message, so history can show it
                // to a phone that was not watching the run.
                let asked = conversation
                    .entries()
                    .iter()
                    .find(|entry| entry.timestamp >= turn_from && entry.is_user());
                if let Some(asked) = asked
                    && let Err(error) = user
                        .record_memory_score(
                            &run.session_id,
                            asked.timestamp.timestamp_millis(),
                            probability,
                        )
                        .await
                {
                    log::warn!("unable to record memory score: {error}; run_id={}", run.id);
                }
            }
            // Without a score (Jev not configured or failing) every turn is read.
            let learned = match score {
                Some(probability) if probability < MEMORY_THRESHOLD => Vec::new(),
                _ => user.learn(generator, exchange, &known).await,
            };
            if let Some(probability) = score {
                log::info!(
                    "memory score {probability:.2}, {} change(s); run_id={}",
                    learned.len(),
                    run.id
                );
            }
            if !learned.is_empty() {
                let calls: Vec<ToolCall> = learned
                    .iter()
                    .map(|change| ToolCall {
                        id: Uuid::new_v4().to_string(),
                        name: change.change.tool().to_owned(),
                        arguments: change.arguments(),
                    })
                    .collect();
                conversation.add(ConversationEntry::assistant_with_tool_calls(
                    "",
                    calls.clone(),
                    None,
                ));
                for (call, change) in calls.into_iter().zip(&learned) {
                    live.push(RunEvent::ToolStarted {
                        name: call.name.clone(),
                        call_id: call.id.clone(),
                        description: change.content.clone(),
                    });
                    live.push(RunEvent::ToolResult {
                        name: call.name,
                        call_id: call.id.clone(),
                        output: change.output(),
                        success: true,
                    });
                    conversation.add(ConversationEntry::tool(call.id, change.output()));
                }
            }
        }

        // Save the turn before clients see the run finish, so they can load it.
        if !failed_tools.is_empty()
            && let Err(error) = user
                .record_tool_failures(&run.session_id, failed_tools)
                .await
        {
            log::warn!("unable to record tool failures: {error}; run_id={}", run.id);
        }
        if let Some(task) = describing {
            match task.await {
                Ok(descriptions) => images::replace_with_descriptions(
                    &mut conversation,
                    turn_started,
                    &descriptions,
                ),
                Err(error) => {
                    log::warn!("image description task failed: {error}; run_id={}", run.id)
                }
            }
        }
        transcript.entries_mut().extend(
            conversation
                .entries()
                .iter()
                .filter(|entry| entry.timestamp >= turn_started && !entry.is_summary())
                .cloned(),
        );
        // Kept before the turn is saved, so the history the phone reloads on
        // the change notice already names the model.
        let asked = transcript
            .entries()
            .iter()
            .find(|entry| entry.timestamp >= turn_started && entry.is_user());
        if let Some(asked) = asked
            && let Err(error) = user
                .record_turn_model(
                    &run.session_id,
                    asked.timestamp.timestamp_millis(),
                    &model.to_string(),
                )
                .await
        {
            log::warn!(
                "unable to record the turn's model: {error}; run_id={}",
                run.id
            );
        }
        let turn = FinishedTurn {
            transcript: &transcript,
            context: &conversation,
            prompt_tokens,
        };
        user.finish(run, Some(turn), status).await?;
        log::info!(
            "turn timing; run_id={} status={} model={model} {timer}",
            run.id,
            status.as_str()
        );
        // Asked only once the turn is saved, so the answer finds the open call.
        if let Some(parked) = parked {
            self.park(user, run, live, parked);
        }
        live.push(RunEvent::RunFinished {
            status: status.as_str().to_owned(),
        });
        Ok(())
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

    /// Asks Jev how likely the exchange holds something worth remembering.
    /// Shown under the reply while we compare it with what the learner saves.
    async fn memory_score(&self, state: String) -> Option<f64> {
        let decision = self.tools.decision.as_ref()?;
        match decision.probability(&state, MEMORY_QUESTION).await {
            Ok(probability) => Some(probability),
            Err(error) => {
                log::warn!("memory score failed: {error}");
                None
            }
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

    /// Conversation starters for the empty chat, personalized and cached
    /// until they go stale, unless `refresh`. Never fails: falls back to fixed
    /// starters.
    pub async fn suggest_prompts(&self, user: &User, refresh: bool) -> PromptSuggestions {
        let here = user.get_location().await.ok().flatten();
        if !refresh && let Some(cached) = user.cached_starters(here.as_ref()).await {
            return cached;
        }
        let context = self.starter_context(user, here.clone());
        user.generate_starters(self.generator(user.id()), context, here)
            .await
    }

    async fn starter_context(&self, user: &User, location: Option<Location>) -> StarterContext {
        let today = user.today().await;
        let (memories, sessions, apps) =
            tokio::join!(user.memory_rows(), user.sessions(), user.apps());
        let memories = memories
            .unwrap_or_default()
            .into_iter()
            .filter(|memory| !memory.valid.ended(today))
            .map(|memory| MemoryNote {
                category: memory.category,
                content: memory.content,
            })
            .collect();
        let recent_titles = sessions
            .unwrap_or_default()
            .into_iter()
            .map(|session| session.title)
            .collect();
        let timezone = location
            .as_ref()
            .and_then(|location| location.timezone.parse::<Tz>().ok());
        let place = location.and_then(|location| location.place);
        let connected_apps = apps
            .unwrap_or_default()
            .into_iter()
            .filter(|app| app.connected)
            .map(|app| app.name)
            .collect();
        StarterContext {
            memories,
            recent_titles,
            now: LocalTime {
                time: Utc::now(),
                timezone,
            },
            place,
            connected_apps,
        }
    }
}

/// What Jev reads to score a turn: what is already remembered, then the
/// exchange, so a fact that is already known does not score high.
fn memory_state(known: &[Hit], exchange: &Exchange) -> String {
    let mut state = String::from("Already remembered:\n");
    if known.is_empty() {
        state.push_str("(nothing related)\n");
    }
    for hit in known {
        let _ = writeln!(state, "- {}", hit.content);
    }
    let latest = &exchange.latest;
    let _ = write!(
        state,
        "\nUser: {}\n\nAssistant: {}",
        latest.message, latest.reply
    );
    state
}

/// The last few turns before `turn_started`, oldest first, for the memory
/// extractor to resolve references in the latest one.
fn earlier_turns(conversation: &Conversation, turn_started: DateTime<Utc>) -> Vec<Turn> {
    let mut turns: Vec<Turn> = Vec::new();
    for entry in conversation.entries() {
        if entry.timestamp >= turn_started {
            break;
        }
        if entry.is_user() {
            turns.push(Turn {
                message: entry.content().to_owned(),
                reply: String::new(),
            });
        } else if entry.is_assistant()
            && !entry.content().is_empty()
            && let Some(turn) = turns.last_mut()
        {
            if !turn.reply.is_empty() {
                turn.reply.push('\n');
            }
            turn.reply.push_str(entry.content());
        }
    }
    let skip = turns.len().saturating_sub(memories::LEARN_CONTEXT_TURNS);
    turns.split_off(skip)
}

/// Firecrawl's page tools. Each run gets its own cache, so `web_search_read`
/// only reaches pages this run crawled.
fn register_web_tools(registry: &mut ToolRegistry, key: &str) {
    let cache: SearchCacheRef = Arc::new(RwLock::new(SearchResultCache::new()));
    registry.register(Arc::new(WebScrapeTool::new(key.to_owned())));
    registry.register(Arc::new(WebMapTool::new(key.to_owned())));
    registry.register(Arc::new(WebCrawlTool::new(key.to_owned(), cache.clone())));
    registry.register(Arc::new(WebExtractTool::new(key.to_owned())));
    registry.register(Arc::new(WebSearchReadTool::new(cache)));
}

/// `Location: Shinjuku, Tokyo, Japan (35.6938 N, 139.7034 E, 3 min ago)`.
/// The age tells the model whether "nearby" still means where the user is.
/// Longest notification body, in characters; the full reply stays in the chat.
const NOTICE_CHARS: usize = 400;

/// A reply as notification text: emphasis and code marks dropped, since the
/// notification shows them literally, and cut to `NOTICE_CHARS`.
fn notification_text(reply: &str) -> String {
    let plain = reply.replace("**", "").replace("__", "").replace('`', "");
    let plain = plain.trim();
    match plain.char_indices().nth(NOTICE_CHARS) {
        Some((end, _)) => format!("{}...", plain[..end].trim_end()),
        None => plain.to_owned(),
    }
}

fn location_line(loc: &Location, now: DateTime<Utc>) -> String {
    let ns = if loc.latitude >= 0.0 { "N" } else { "S" };
    let ew = if loc.longitude >= 0.0 { "E" } else { "W" };
    let coords = format!(
        "{:.4} {ns}, {:.4} {ew}",
        loc.latitude.abs(),
        loc.longitude.abs()
    );
    let minutes = (now - loc.updated_at).num_minutes().max(0);
    let age = match minutes {
        0 => "just now".to_owned(),
        1..60 => format!("{minutes} min ago"),
        60..2880 => format!("{} h ago", minutes / 60),
        _ => format!("{} days ago", minutes / 1440),
    };
    match &loc.place {
        Some(place) => format!("Location: {place} ({coords}, {age})"),
        None => format!("Location: {coords} ({age})"),
    }
}

/// Which apps are connected and which the user could connect, so the model
/// sends them to the Apps screen instead of guessing how accounts are linked.
fn apps_line(apps: &[AppIntegration]) -> Option<String> {
    let names = |connected: bool| {
        apps.iter()
            .filter(|app| app.configured && app.connected == connected)
            .map(|app| app.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let (connected, available) = (names(true), names(false));
    if connected.is_empty() && available.is_empty() {
        return None;
    }
    let mut line = format!(
        "Connected apps: {}.",
        if connected.is_empty() {
            "none"
        } else {
            &connected
        }
    );
    if !available.is_empty() {
        line.push_str(&format!(
            " Not connected: {available}. The user connects these from Apps in Beep's menu."
        ));
    }
    Some(line)
}

fn foreground_line(app: &ForegroundApp) -> String {
    format!(
        "Open app: {} ({}). The user is chatting from the bubble over this app; \
         read_screen shows what is on it.",
        app.name, app.package
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::AppKind;

    fn app(name: &str, connected: bool, configured: bool) -> AppIntegration {
        AppIntegration {
            slug: name.to_lowercase(),
            name: name.into(),
            description: String::new(),
            logo_url: None,
            connected,
            configured,
            kind: AppKind::Composio {},
        }
    }

    #[test]
    fn apps_line_names_connected_and_available_apps() {
        let apps = [
            app("Fastmail", true, true),
            app("Outlook", false, true),
            app("Teams", false, true),
            app("Slack", false, false),
        ];
        assert_eq!(
            apps_line(&apps).as_deref(),
            Some(
                "Connected apps: Fastmail. Not connected: Outlook, Teams. \
                 The user connects these from Apps in Beep's menu."
            )
        );
        assert_eq!(
            apps_line(&[app("Gmail", false, true)]).as_deref(),
            Some(
                "Connected apps: none. Not connected: Gmail. \
                 The user connects these from Apps in Beep's menu."
            )
        );
        assert_eq!(apps_line(&[app("Slack", false, false)]), None);
    }

    fn fix(place: Option<&str>, updated_at: DateTime<Utc>) -> Location {
        Location {
            latitude: 35.69384,
            longitude: 139.70341,
            accuracy: 12.0,
            timezone: "Asia/Tokyo".into(),
            place: place.map(Into::into),
            updated_at,
        }
    }

    #[test]
    fn location_line_names_the_place_and_its_age() {
        let now = Utc::now();
        let loc = fix(
            Some("Shinjuku, Tokyo, Japan"),
            now - Duration::from_secs(180),
        );
        assert_eq!(
            location_line(&loc, now),
            "Location: Shinjuku, Tokyo, Japan (35.6938 N, 139.7034 E, 3 min ago)"
        );
    }

    #[test]
    fn location_line_without_a_place_keeps_the_coordinates() {
        let now = Utc::now();
        let loc = fix(None, now - Duration::from_secs(3 * 3600));
        assert_eq!(
            location_line(&loc, now),
            "Location: 35.6938 N, 139.7034 E (3 h ago)"
        );
        assert!(location_line(&fix(None, now), now).ends_with("(just now)"));
    }

    #[test]
    fn notification_text_is_plain_and_short() {
        assert_eq!(
            notification_text("  Time to leave: **check-in** closes at `11:45`.\n"),
            "Time to leave: check-in closes at 11:45."
        );
        let long = "a".repeat(NOTICE_CHARS + 10);
        assert_eq!(
            notification_text(&long),
            format!("{}...", "a".repeat(NOTICE_CHARS))
        );
        assert_eq!(notification_text("  "), "");
    }
}
