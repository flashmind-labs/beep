use crate::{
    apps::{Composio, ComposioSettings},
    compaction::{self, Summarizer},
    config::Auth,
    decision::DecisionTool,
    live::LiveRun,
    memories::{self, Exchange, Turn},
    protocol::{DeviceAnswer, DeviceQuery, Notice, PromptSuggestions, Run, RunEvent, Usage},
    repeat::RepeatGuard,
    search::SearchTool,
    starters::{Generator, LocalTime, MemoryNote, StarterContext},
    store::{CreateRun, FinishedTurn, Location, RunStatus, SessionRef, Store},
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
        AgentEvent, AgentInput, AgentLlmConfig, ContentPart, LlmProvider, ToolCall, ToolRegistry,
    },
};
use futures::StreamExt;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use std::{collections::HashMap, fmt::Write, sync::Arc, time::Duration};

/// How long a run may go without a single event from the model or a tool.
const IDLE_TIMEOUT: Duration = Duration::from_secs(90);
/// The same while a tool runs: crawls and extractions poll quietly for up to
/// two minutes. The run's overall deadline still applies.
const TOOL_IDLE_TIMEOUT: Duration = Duration::from_secs(150);
/// Above this Jev score the learner reads the turn; below it nothing is stored.
const MEMORY_THRESHOLD: f64 = 0.8;
const MEMORY_QUESTION: &str = "Does this exchange reveal, correct or ask to forget a lasting fact about the user worth remembering for future chats, such as their name, preferences, people, places, work, routines or plans, whether the user said it or the assistant found it in their data at their request? Answer no for small talk, one-off questions, or facts only about the world.";
use tokio::sync::{Mutex, RwLock, broadcast, oneshot};
use uuid::Uuid;

/// The server's shared state: storage, users, the model and the
/// integrations every request and background task works through.
pub struct App {
    pub store: Store,
    /// The users allowed in and their tokens.
    pub auth: Auth,
    /// Notices for the phones; each stream keeps only its user's.
    pub notifications: broadcast::Sender<UserNotice>,
    /// The model behind runs, titles and starters.
    provider: Arc<dyn LlmProvider>,
    /// Model, reasoning and token budget for every run; each run adds its user.
    llm: AgentLlmConfig,
    /// The optional tools each run gets.
    tools: RunTools,
    /// Every configured user, by id.
    users: HashMap<String, Arc<User>>,
    /// Runs in progress, with their event feed.
    runs: Mutex<HashMap<String, Arc<LiveRun>>>,
    /// Questions sent to a phone, keyed by query id, waiting for its answer.
    device_queries: Mutex<HashMap<String, oneshot::Sender<DeviceAnswer>>>,
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
}

/// The part of `ServerTools` a run registers itself.
struct RunTools {
    search: Option<Arc<SearchTool>>,
    firecrawl_key: Option<SecretString>,
    decision: Option<Arc<DecisionTool>>,
}

/// A `Notice` addressed to one user's open notification streams.
#[derive(Debug, Clone)]
pub struct UserNotice {
    pub user: String,
    pub notice: Notice,
}

#[derive(Deserialize)]
pub struct Message {
    pub session_id: Uuid,
    pub request_id: Uuid,
    pub message: String,
    #[serde(default)]
    pub cron_id: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
}

impl App {
    pub fn new(
        store: Store,
        auth: Auth,
        provider: Arc<dyn LlmProvider>,
        llm: AgentLlmConfig,
        tools: ServerTools,
    ) -> Arc<Self> {
        let ServerTools {
            search,
            firecrawl_key,
            decision,
            composio,
        } = tools;
        let (notifications, _) = broadcast::channel(64);
        let shared = Shared {
            store: store.clone(),
            notifications: notifications.clone(),
            composio: composio.map(|settings| Arc::new(Composio::new(settings))),
        };
        let users = auth
            .user_ids()
            .map(|id| (id.to_owned(), User::new(id, &shared)))
            .collect();
        Arc::new(Self {
            store,
            auth,
            notifications,
            provider,
            llm,
            tools: RunTools {
                search,
                firecrawl_key,
                decision,
            },
            users,
            runs: Mutex::new(HashMap::new()),
            device_queries: Mutex::new(HashMap::new()),
        })
    }

    /// The configured user with `id`.
    pub fn user(&self, id: &str) -> Option<&Arc<User>> {
        self.users.get(id)
    }

    /// Every configured user.
    pub fn users(&self) -> impl Iterator<Item = &Arc<User>> {
        self.users.values()
    }

    pub async fn submit(self: &Arc<Self>, user: &Arc<User>, message: Message) -> Result<CreateRun> {
        // Serialize registration with cancellation so a newly created run can always be stopped.
        let mut runs = self.runs.lock().await;
        let result = user
            .create_run(
                &message.session_id.to_string(),
                &message.request_id.to_string(),
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
            let is_cron = message.cron_id.is_some();
            let image = message.image.clone();
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
                        titles::name_first(&app.store, generator, &session, &text).await;
                    });
                }
                Ok(false) => {}
                Err(error) => log::warn!("unable to check title: {error}; run_id={}", run.id),
            }
            tokio::spawn(async move {
                if let Err(error) = app
                    .execute(
                        &user,
                        &run,
                        &message.message,
                        image.as_deref(),
                        is_cron,
                        &live,
                    )
                    .await
                {
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

    pub async fn notify_run_complete(&self, user: &User, run: &Run, status: RunStatus) {
        // Build a title from the session's first message (truncated).
        let title = match user.sessions().await {
            Ok(sessions) => sessions
                .into_iter()
                .find(|s| s.id == run.session_id)
                .map(|s| s.title)
                .unwrap_or_else(|| "Run finished".to_owned()),
            Err(_) => "Run finished".to_owned(),
        };
        let body = match status {
            RunStatus::Completed => "Your request completed successfully.",
            RunStatus::Cancelled => "The run was cancelled.",
            RunStatus::TimedOut => "The run timed out.",
            RunStatus::Stalled => "The model stopped responding.",
            RunStatus::Looped => "The model got stuck repeating itself and was stopped.",
            RunStatus::ToolLimit => "The run used too many tools and was stopped.",
            RunStatus::Failed => "The run failed.",
        }
        .to_owned();
        let notice = UserNotice {
            user: user.id().to_owned(),
            notice: Notice {
                title,
                body,
                session_id: run.session_id.clone(),
                app: None,
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

    /// Time and location for this turn, sent as a developer message. Memories are
    /// not included: the model looks them up with `memory_recall`.
    async fn build_context(&self, user: &User) -> Option<String> {
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

        (!ctx.is_empty()).then_some(ctx)
    }

    /// Parse a `data:image/jpeg;base64,...` URI into a `ContentPart::Image`.
    fn parse_image_data_uri(uri: &str) -> Option<ContentPart> {
        let rest = uri.strip_prefix("data:")?;
        let (header, data) = rest.split_once(",")?;
        let media_type = header.strip_suffix(";base64")?;
        if !media_type.starts_with("image/") {
            return None;
        }
        Some(ContentPart::Image {
            media_type: media_type.to_owned(),
            data: data.to_owned(),
        })
    }

    async fn execute(
        self: &Arc<Self>,
        user: &Arc<User>,
        run: &Run,
        message: &str,
        image: Option<&str>,
        is_cron: bool,
        live: &LiveRun,
    ) -> Result<()> {
        let cancel = &live.cancel;

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
        let context = self.build_context(user).await;

        let llm = self.llm.clone().with_user(user.id());

        let mut registry = tools::registry(self.clone(), &run.id, user);
        if let Some(search) = &self.tools.search {
            registry.register(search.clone());
        }
        if let Some(key) = &self.tools.firecrawl_key {
            register_web_tools(&mut registry, key.expose_secret());
        }
        if let Some(decision) = &self.tools.decision {
            registry.register(decision.clone());
        }

        let (composio, mcp) = tokio::join!(user.app_tools(), user.mcp_tools());
        for tool in composio.into_iter().chain(mcp) {
            registry.register(tool);
        }

        let mut agent = Agent::builder(self.provider.clone())
            .llm(llm)
            .tools(registry)
            .build_sync();

        // Flashmind saves `context` as a developer message before the user message. It
        // stays in history, which keeps the cached prefix intact and timestamps each turn.
        let input = AgentInput::User {
            content: message.to_owned(),
            context,
            parts: image
                .and_then(Self::parse_image_data_uri)
                .map(|part| vec![part]),
        };

        // Conversation entries do not record whether a tool failed; history needs it.
        let mut failed_tools = Vec::new();
        // The reply the memory extractor reads once the run completes.
        let mut reply = String::new();
        // Prompt size of the last model call, which decides later compaction.
        let mut prompt_tokens = 0;
        // Entries from here on belong to this turn. Matching by time rather than
        // index survives the agent compacting mid-run.
        let turn_started = Utc::now();
        let status = {
            let stream = agent.start(&mut conversation, cancel.clone(), input, None);
            tokio::pin!(stream);

            let deadline = tokio::time::sleep(Duration::from_secs(180));
            tokio::pin!(deadline);

            let mut tools_started = 0;
            let mut tools_running = 0usize;
            // A model repeating itself never stops on its own; end the run instead
            // of streaming the same sentence until the deadline.
            let mut text_guard = RepeatGuard::default();
            let mut reasoning_guard = RepeatGuard::default();
            loop {
                let event = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => break RunStatus::Cancelled,
                    _ = &mut deadline => { cancel.cancel(); break RunStatus::TimedOut; },
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
                        reply.push_str(&delta);
                        RunEvent::TextDelta { delta }
                    }
                    Some(AgentEvent::ReasoningDelta(delta)) => {
                        if reasoning_guard.push(&delta) {
                            cancel.cancel();
                            break RunStatus::Looped;
                        }
                        RunEvent::ReasoningDelta { delta }
                    }
                    Some(AgentEvent::ToolStart {
                        name,
                        id,
                        humanized,
                    }) => {
                        tools_started += 1;
                        tools_running += 1;
                        if tools_started > 20 {
                            cancel.cancel();
                            break RunStatus::ToolLimit;
                        }
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
                        if !success {
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
            }
        };

        if is_cron {
            // Notify registered devices for cron-triggered runs only.
            self.notify_run_complete(user, run, status).await;
            // Log cron completion.
            let _ = self.store.log_cron_finish(&run.id, status, "").await;
        }

        // Learn from the turn while the run is still open, so what was saved shows
        // as memory tool chips under the reply. Scheduled prompts are written by
        // the user once, not said in conversation.
        if status == RunStatus::Completed && !is_cron {
            let generator = Generator {
                provider: self.provider.as_ref(),
                llm: &self.llm,
                user: user.id(),
            };
            let score = self
                .memory_score(format!("User: {message}\n\nAssistant: {reply}"))
                .await;
            if let Some(probability) = score {
                live.push(RunEvent::MemoryScore { probability });
                // Kept by the time of the turn's user message, so history can show it
                // to a phone that was not watching the run.
                let asked = conversation
                    .entries()
                    .iter()
                    .find(|entry| entry.timestamp >= turn_started && entry.is_user());
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
            let exchange = Exchange {
                today: user.today().await,
                earlier: earlier_turns(&conversation, turn_started),
                latest: Turn {
                    message: message.to_owned(),
                    reply,
                },
            };
            // Without a score (Jev not configured or failing) every turn is read.
            let learned = match score {
                Some(probability) if probability < MEMORY_THRESHOLD => Vec::new(),
                _ => user.learn(generator, exchange).await,
            };
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
        transcript.entries_mut().extend(
            conversation
                .entries()
                .iter()
                .filter(|entry| entry.timestamp >= turn_started && !entry.is_summary())
                .cloned(),
        );
        let turn = FinishedTurn {
            transcript: &transcript,
            context: &conversation,
            prompt_tokens,
        };
        user.finish(run, Some(turn), status).await?;
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
        titles::retitle_idle(&self.store, self.provider.as_ref(), &self.llm).await;
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

    /// Conversation starters for the empty chat, personalized and cached for
    /// 12 hours unless `refresh`. Never fails: falls back to fixed starters.
    pub async fn suggest_prompts(&self, user: &User, refresh: bool) -> PromptSuggestions {
        if !refresh && let Some(cached) = user.cached_starters().await {
            return cached;
        }
        user.generate_starters(self.generator(user.id()), self.starter_context(user))
            .await
    }

    async fn starter_context(&self, user: &User) -> StarterContext {
        let today = user.today().await;
        let (memories, sessions, location, apps) = tokio::join!(
            user.memory_rows(),
            user.sessions(),
            user.get_location(),
            user.apps(),
        );
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
            .ok()
            .flatten()
            .and_then(|location| location.timezone.parse::<Tz>().ok());
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
            connected_apps,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
