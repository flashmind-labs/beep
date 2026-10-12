//! Runs one turn of the agent and saves it.

use super::{
    App, Message,
    auto::{AgentTurn, Prompt},
    device::Parked,
    learning::{MEMORY_THRESHOLD, ServerCall, add_server_calls, earlier_turns, memory_state},
};
use crate::{
    agents::AgentId,
    apps::markdown,
    followups, gates, images,
    live::LiveRun,
    memories::{Exchange, Turn},
    permissions::{self, ApprovalPayload, Approved, Rules},
    protocol::{ApprovalDecision, AutoPick, Run, RunEvent, Usage},
    repeat::{self, RepeatGuard},
    sandbox,
    steps::{self, TurnsByTime},
    store::{FinishedTurn, PicturesOf, RunStatus, SessionRef, Step, StepBody, TurnRecord},
    timing::{Output, TurnTimer},
    tools,
    user::User,
    wake,
};
use anyhow::Result;
use chrono::Utc;
use flashmind::{
    core::{Agent, Conversation, ConversationEntry},
    tools::{
        firecrawl::{WebCrawlTool, WebExtractTool, WebMapTool, WebScrapeTool},
        search_cache::{SearchCacheRef, SearchResultCache},
        search_read::WebSearchReadTool,
    },
    types::{
        AgentEvent, AgentInput, ContentPart, ToolRegistry,
        tool::{Tool, ToolResult},
    },
};
use futures::StreamExt;
use secrecy::ExposeSecret;
use std::{
    collections::HashSet,
    mem,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime},
};
use tokio::{
    sync::RwLock,
    time::{Instant, sleep, timeout},
};
use uuid::Uuid;

/// The longest any run may take. A model stuck repeating itself is stopped
/// sooner by the repeat guards.
const MAX_RUN: Duration = Duration::from_secs(15 * 60);
/// How long a run may go without a single event from the model or a tool.
const IDLE_TIMEOUT: Duration = Duration::from_secs(90);
/// The same while a tool runs: crawls and extractions poll quietly for up to
/// two minutes. The run's overall deadline still applies.
const TOOL_IDLE_TIMEOUT: Duration = Duration::from_secs(150);

impl App {
    pub(super) async fn execute(
        self: &Arc<Self>,
        user: &Arc<User>,
        run: &Run,
        message: &Message,
        live: &LiveRun,
    ) -> Result<()> {
        let cancel = &live.cancel;
        let is_cron = message.cron_id.is_some();
        // Nobody asked for a routine's or an event's turn.
        let background = is_cron || message.woken.is_some();
        let mut timer = TurnTimer::start();
        if !background && message.answer.is_none() {
            user.forget_used_starter(&message.message).await;
        }

        let chat = Uuid::parse_str(&run.session_id)?;
        // The model runs on the context, which may be compacted. The apps
        // read the turn's steps, saved as messages when it ends.
        let mut conversation = user
            .context(chat)
            .await?
            .map_or_else(Conversation::new, |context| context.conversation);

        let answer = message.answer.as_ref();
        let prompt = Prompt {
            message: &message.message,
            images: message.images.len(),
            conversation: &conversation,
            session: chat,
        };
        // With agents on, every chat turn runs as one: Jev picks it, and a
        // resumed turn keeps the one it started as. Routines run as General.
        let turn_agent = match answer {
            _ if background || !self.agents_on().await => None,
            Some(_) => self.resumed_agent(user, chat).await.map(|agent| AgentTurn {
                wanted: agent.thinking,
                agent,
            }),
            None => Some(self.agent_pick(user, &prompt).await),
        };
        // An admin's one-off model or thinking wins over Auto and the agent;
        // a resumed turn keeps the pick it started with. Auto picks for chats
        // only: a routine runs on the admin's selected model.
        let one_off = message.model.is_some() || message.thinking.is_some();
        let auto = match (one_off, answer, &turn_agent) {
            (true, ..) => None,
            (false, Some(_), _) => self.resumed_pick(user, chat).await,
            (false, None, _) if background => None,
            (false, None, Some(agent)) => {
                if self.agent_picks_model(user).await {
                    Some(agent.choice())
                } else {
                    None
                }
            }
            (false, None, None) => self.auto_pick(user, &prompt).await,
        };
        // Always set the server-owned prompt, including when loading an older
        // conversation. It stays static per agent so the provider can cache it.
        conversation.set_system(
            turn_agent
                .as_ref()
                .map_or(AgentId::General, |turn| turn.agent.id)
                .prompt(),
        );
        let (model, thinking) = match &auto {
            Some(auto) => (auto.model.clone(), auto.thinking),
            None => (
                match message.model.clone() {
                    Some(model) => model,
                    None => self.model_for(user).await,
                },
                match message.thinking {
                    Some(thinking) => thinking,
                    None => self.thinking_for(user).await,
                },
            ),
        };
        if let Some(auto) = &auto {
            live.push(RunEvent::AutoPicked {
                pick: AutoPick {
                    model: auto.model.to_string(),
                    thinking: auto.thinking,
                },
            });
        }
        let mut llm = self
            .llm
            .with_model(model.clone())
            .with_user(user.id())
            .with_reasoning(thinking)
            // One ID per chat keeps OpenRouter on the provider holding its cache.
            .with_session_id(chat);
        if let Some(routing) = self.models.routing(&model) {
            llm.provider_preferences = Some(routing.clone());
        }

        let timezone = user.get_timezone().await.ok();
        let mut registry = tools::registry(
            self.clone(),
            run,
            user,
            timezone.as_deref(),
            message.origin,
            message.charts,
        );
        if let Some(search) = &self.tools.search {
            registry.register(search.clone());
        }
        if let Some(key) = &self.tools.firecrawl_key {
            register_web_tools(&mut registry, key.expose_secret());
        }
        if let Some(decision) = &self.tools.decision {
            registry.register(decision.clone());
        }
        if let Some(transitous) = &self.tools.transitous {
            registry.register(Arc::new(tools::GetRouteTool {
                user: user.clone(),
                transitous: transitous.clone(),
            }));
        }
        if let Some(sandbox) = &self.tools.sandbox {
            sandbox::register(&mut registry, sandbox, user);
        }
        registry.register(Arc::new(tools::SendFileTool {
            user: user.clone(),
            session_id: chat,
            sandbox: self.tools.sandbox.clone(),
        }));
        registry.register(Arc::new(tools::SendImageTool {
            user: user.clone(),
            session_id: chat,
            sandbox: self.tools.sandbox.clone(),
        }));
        let notify = Arc::new(AtomicBool::new(false));
        if background {
            registry.register(Arc::new(tools::NotifyUserTool {
                notify: notify.clone(),
            }));
        }

        // Calls of the last model turn still without a result: the session
        // stopped on the first one to ask the user.
        let open = permissions::open_calls(&conversation);
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
        let tools = markdown::wrap(composio.tools.into_iter().chain(mcp).collect());
        for tool in permissions::guard(tools, rules, approved, &composio.hinted) {
            registry.register(tool);
        }
        let refusals = repeat::guard_tool_calls(&mut registry);

        // Entries from here on belong to this turn. Matching by time rather than
        // index survives the agent compacting mid-run.
        let turn_started = Utc::now();
        // Conversation entries do not record whether a tool failed; the apps need it.
        let mut failed_tools = HashSet::new();
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
                    failed_tools.insert(call.id.clone());
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
            .build_context(user, &conversation, message, &apps.unwrap_or_default())
            .await;
        timer.context_built(building.elapsed());

        let mut agent = Agent::builder(self.provider.clone())
            .llm(llm)
            .tools(registry)
            .allow_parallel_tool_calls(true)
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
            let hard_stop = sleep(MAX_RUN);
            tokio::pin!(hard_stop);

            let mut input = input;
            // The turn's calls, which the gates read with the reply.
            let mut calls = Vec::new();
            let mut gated = false;
            loop {
                let status = {
                    let stream = agent.start(
                        &mut conversation,
                        cancel.clone(),
                        mem::take(&mut input),
                        None,
                    );
                    tokio::pin!(stream);

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
                            _ = &mut hard_stop => { cancel.cancel(); break RunStatus::TimedOut; },
                            event = timeout(
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
                                if !success {
                                    failed_tools.insert(id.clone());
                                }
                                calls.push(gates::Call {
                                    name: name.clone(),
                                    success,
                                });
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
                                // A parallel batch can stop on several calls; ask
                                // about the first; resuming reaches the others.
                                if parked.is_some() {
                                    continue;
                                }
                                parked = payload
                                    .as_ref()
                                    .and_then(|payload| {
                                        payload.as_any().downcast_ref::<ApprovalPayload>()
                                    })
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
                // A finished reply is checked once; a failed check gets the
                // agent one more pass to fix it.
                if gated || status != RunStatus::Completed || parked.is_some() {
                    break status;
                }
                gated = true;
                let Some(decision) = &self.tools.decision else {
                    break status;
                };
                let draft = gates::Draft {
                    asked: &message.message,
                    reply: &reply,
                    calls: &calls,
                };
                let Some(verdict) = gates::review(decision, &draft).await else {
                    break status;
                };
                conversation.add(ConversationEntry::system_message(verdict.note));
            }
        };

        timer.streamed();
        if parked.is_some() && status == RunStatus::Completed {
            status = RunStatus::Waiting;
        }

        // A routine that found nothing to say leaves no trace in the chat. Its
        // turn stays in the context, so the next run knows what this one saw.
        let silent = is_cron && status == RunStatus::Completed && !notify.load(Ordering::SeqCst);
        // A run that stops to ask notifies with the question instead.
        if is_cron && status != RunStatus::Waiting && !silent {
            self.notify_run_complete(user, run, status, &reply).await;
        }

        // A resumed turn counts from the message that started it.
        let asked = answer.and_then(|_| {
            conversation
                .entries()
                .iter()
                .rev()
                .find(|entry| entry.timestamp < turn_started && entry.is_user())
        });
        let turn_from = asked.map_or(turn_started, |entry| entry.timestamp);
        // What the turn did in the user's apps, so starters stop suggesting it.
        let actions = permissions::actions_since(&conversation, turn_from);
        if !actions.is_empty() {
            if let Err(error) = user.record_actions(&actions).await {
                log::warn!("unable to record actions: {error}; run_id={}", run.id);
            }
            user.forget_starters().await;
        }

        // Learn from the turn while the run is still open, so what was saved shows
        // as memory tool chips under the reply. Scheduled prompts are written by
        // the user once, not said in conversation.
        // Jev's memory score for the turn, kept with it.
        let mut memory_score = None;
        if status == RunStatus::Completed && !background {
            let generator = self.generator(user.id());
            let exchange = Exchange {
                session_id: run.session_id.clone(),
                today: user.today().await,
                earlier: earlier_turns(&conversation, turn_from),
                latest: Turn {
                    message: asked.map_or_else(
                        || message.message.clone(),
                        |entry| entry.content().to_owned(),
                    ),
                    reply: reply.clone(),
                    actions,
                },
            };
            // Written alongside learning.
            let follow_up = self.decision().map(|decision| {
                (
                    decision,
                    followups::Input::new(
                        &conversation,
                        turn_from,
                        &exchange.earlier,
                        &exchange.latest,
                    )
                    .to_string(),
                )
            });
            let kind = followups::Kind::of(&exchange.latest.reply);
            let replied = exchange.latest.reply.clone();
            let follow_up = async {
                let (decision, state) = follow_up?;
                followups::write(decision, generator, state, kind, &replied).await
            };
            let learning = async {
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
                }
                // Without a score (Jev not configured or failing) every turn is read,
                // and so is one that acted, since it may finish what a memory plans.
                let acted = !exchange.latest.actions.is_empty();
                let learned = match score {
                    Some(probability) if probability < MEMORY_THRESHOLD && !acted => Vec::new(),
                    _ => user.learn(generator, exchange, &known).await,
                };
                if let Some(probability) = score {
                    log::info!(
                        "memory score {probability:.2}, {} change(s); run_id={}",
                        learned.len(),
                        run.id
                    );
                }
                (learned, score)
            };
            let ((learned, score), follow_up) = tokio::join!(learning, follow_up);
            memory_score = score;
            let mut calls: Vec<ServerCall> = learned
                .iter()
                .map(|change| ServerCall {
                    name: change.change.tool().to_owned(),
                    arguments: change.arguments(),
                    description: change.content.clone(),
                    output: change.output(),
                })
                .collect();
            if let Some(output) = follow_up {
                calls.push(ServerCall {
                    name: tools::SUGGEST_REPLIES.to_owned(),
                    arguments: serde_json::json!({}),
                    description: tools::SUGGESTING_REPLIES.to_owned(),
                    output,
                });
            }
            add_server_calls(&mut conversation, live, calls);
        }

        // Save the turn before clients see the run finish, so they can load it.
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
        // What the turn ran with sits on its opening message; a resumed turn
        // adds to the one it carries on.
        let record = TurnRecord {
            model: model.to_string(),
            auto_thinking: auto.as_ref().map(|auto| auto.thinking),
            agent: turn_agent.as_ref().map(|turn| turn.agent.id),
            reasoning: String::new(),
            memory_score: if answer.is_none() { memory_score } else { None },
        };
        let mut turns = TurnsByTime::new();
        if let Some(asked) = conversation
            .entries()
            .iter()
            .find(|entry| entry.timestamp >= turn_started && entry.is_user())
        {
            turns.insert(asked.timestamp.timestamp_millis(), record);
        }
        // A routine that found nothing to say shows nothing.
        let mut saved = if silent {
            steps::Steps::default()
        } else {
            steps::steps(
                conversation.entries(),
                Some(turn_started),
                &failed_tools,
                &turns,
            )
        };
        // A call that only names its records, such as a message ID, says what
        // they are, so the user knows what it acts on.
        if let Some(parked) = &mut parked {
            parked
                .approval
                .fields
                .extend(permissions::describe_call(user, &parked.call_id, &conversation).await);
            saved.steps.push(Step {
                sent_at: Utc::now(),
                body: StepBody::Approval {
                    query_id: parked.call_id.clone(),
                    approval: parked.approval.clone(),
                },
            });
        }
        let turn = FinishedTurn {
            context: &conversation,
            prompt_tokens,
            steps: &saved.steps,
            carried: TurnRecord {
                reasoning: saved.carried_reasoning.clone(),
                memory_score: if answer.is_some() { memory_score } else { None },
                ..TurnRecord::default()
            },
        };
        let finished = user.finish(run, Some(turn), status).await?;
        // The pictures stay with the message they came with, so every device
        // shows them after the model only keeps their descriptions.
        if let Some(opened) = finished.and_then(|saved| saved.opened)
            && !message.images.is_empty()
        {
            let of = PicturesOf {
                chat,
                message: opened,
            };
            match user
                .save_pictures(of, &images::pictures(&message.images))
                .await
            {
                Ok(_) => user.changed(&run.session_id),
                Err(error) => log::warn!(
                    "unable to keep the turn's pictures: {error}; run_id={}",
                    run.id
                ),
            }
        }
        // A routine's turns were written once, not asked, so they teach no skill.
        if status == RunStatus::Completed && !background {
            let calls = crate::skills::tool_calls(
                conversation
                    .entries()
                    .iter()
                    .filter(|entry| entry.timestamp >= turn_from),
            );
            self.count_for_skills(user, chat, calls);
        }
        self.compact_after_turn(
            SessionRef {
                user: user.id().to_owned(),
                id: chat,
            },
            model.clone(),
            prompt_tokens,
        );
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
            updated_at: finished.map(|saved| SystemTime::from(saved.updated_at)),
        });
        if let Some(label) = &message.woken {
            let finished = wake::Finished {
                label,
                status,
                notified: notify.load(Ordering::SeqCst),
                reply: &reply,
            };
            wake::finish(self, user, run, finished).await;
        }
        Ok(())
    }
}

/// Firecrawl's page tools. Each run gets its own cache, so `web_search_read`
/// only reaches pages this run crawled.
pub(super) fn register_web_tools(registry: &mut ToolRegistry, key: &str) {
    for tool in web_tools(key) {
        registry.register(tool);
    }
}

/// Firecrawl's page tools, sharing one cache.
pub(super) fn web_tools(key: &str) -> Vec<Arc<dyn Tool>> {
    let cache: SearchCacheRef = Arc::new(RwLock::new(SearchResultCache::new()));
    vec![
        Arc::new(WebScrapeTool::new(key.to_owned())),
        Arc::new(WebMapTool::new(key.to_owned())),
        Arc::new(WebCrawlTool::new(key.to_owned(), cache.clone())),
        Arc::new(WebExtractTool::new(key.to_owned())),
        Arc::new(WebSearchReadTool::new(cache)),
    ]
}
