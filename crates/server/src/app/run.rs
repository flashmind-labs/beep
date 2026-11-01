//! Runs one turn of the agent and saves it.

use super::{
    App, Message,
    auto::Prompt,
    device::Parked,
    learning::{
        MEMORY_THRESHOLD, ServerCall, TurnState, add_server_calls, drop_follow_ups,
        drop_text_since, earlier_turns,
    },
    research,
};
use crate::{
    apps::{files, markdown},
    attachments::{self, Handed},
    cron, events, followups, gates,
    grants::{RequestGrantTool, RoutineGrants},
    images,
    instance::Privilege,
    live::LiveRun,
    memories::{Exchange, Turn},
    metering,
    permissions::{self, ApprovalPayload, Approved, Rules},
    protocol::{ApprovalDecision, AutoPick, Run, RunEvent, Usage},
    repeat::{self, RepeatGuard},
    sources::ReplySources,
    steps::{self, TurnsByTime},
    store::{FinishedTurn, PicturesOf, RunStatus, SessionRef, Step, StepBody, TurnRecord},
    timing::{Output, TurnTimer},
    tool_cost, tools,
    user::User,
    wake,
    workspace::Workspace,
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
/// The system prompt of every chat turn.
const ASSISTANT_PROMPT: &str = include_str!("../../prompts/assistant.md");

const MAX_RUN: Duration = Duration::from_secs(15 * 60);
/// How many times a reply that stopped partway through the user's task is
/// sent back to keep going.
const MAX_CONTINUES: usize = 6;

/// How long a run may go without a single event from the model or a tool.
const IDLE_TIMEOUT: Duration = Duration::from_secs(90);
/// The same while a tool runs: crawls and extractions poll quietly for up to
/// two minutes. The run's overall deadline still applies.
const TOOL_IDLE_TIMEOUT: Duration = Duration::from_secs(150);

/// How much of a scheduled task the reply checks read.
const SCHEDULED_TASK_CHARS: usize = 160;

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
        let background = is_cron || message.woken.is_some() || message.finished_research;
        let mut timer = TurnTimer::start();
        if !background && message.answer.is_none() {
            user.forget_used_starter(&message.message).await;
        }

        let chat = Uuid::parse_str(&run.session_id)?;
        let answer = message.answer.as_ref();
        // A research chat runs as its lead, on the research's own models.
        let research = match self.models.research() {
            Some(config) => user.research_in(chat).await?.map(|row| (row, config)),
            None => None,
        };
        // The model runs on the context, which may be compacted. The apps
        // read the turn's steps, saved as messages when it ends.
        let mut conversation = user
            .context(chat)
            .await?
            .map_or_else(Conversation::new, |context| context.conversation);
        drop_follow_ups(&mut conversation);
        // A routine runs on its own turn alone, not on the chat's history.
        // Its turn is added to the chat's context once it ends, so a reply
        // in the chat knows what it said.
        let chat_context = (is_cron && answer.is_none())
            .then(|| mem::replace(&mut conversation, Conversation::new()));
        // A routine's chat that holds nothing yet was made for this run.
        let fresh_chat = chat_context
            .as_ref()
            .is_some_and(|context| context.entries().is_empty());

        let prompt = Prompt {
            message: &message.message,
            images: message.images.len(),
            conversation: &conversation,
        };
        let one_off = message.model.is_some() || message.thinking.is_some();
        // The picks ask a model, so a stop does not wait for them.
        let picking = async {
            if research.is_some() {
                return None;
            }
            // An admin's one-off model or thinking wins over Auto; a resumed
            // turn keeps the pick it started with. Auto picks for chats only:
            // a routine runs on the user's selected model.
            match (one_off, answer) {
                (true, _) => None,
                (false, Some(_)) => self.resumed_pick(user, chat).await,
                (false, None) if background => None,
                (false, None) => self.auto_pick(user, &prompt).await,
            }
        };
        let Some(auto) = cancel.run_until_cancelled(picking).await else {
            return self.stopped_early(user, run, live).await;
        };
        if research.is_some() {
            metering::research_turn();
        }
        // Always set the server-owned prompt, including when loading an older
        // conversation. It stays the same per user so the provider can cache
        // it: only the user's languages are added.
        let prompt = match &research {
            Some(_) => research::LEAD_PROMPT,
            None => ASSISTANT_PROMPT,
        };
        conversation.set_system(match user.spoken_languages().await {
            Ok(Some(languages)) => format!("{prompt}\n\n{}", languages_line(&languages)),
            Ok(None) => prompt.to_owned(),
            Err(error) => {
                log::warn!("unable to read the user's languages: {error}; user={user}");
                prompt.to_owned()
            }
        });
        let (model, thinking) = match (&auto, &research) {
            (Some(auto), _) => (auto.model.clone(), auto.thinking),
            (None, Some((_, config))) if !one_off => {
                (config.lead.model.clone(), config.lead.thinking)
            }
            _ => (
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
            .with_reasoning(thinking);
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
            message.draws,
        );
        if is_cron {
            cron::remove_changing_tools(&mut registry);
            // A watch ends itself once it has told the user; the digest is
            // always sent and never ends.
            if !message.always_sent
                && let Some(routine) = message.cron_id.as_deref().and_then(|id| id.parse().ok())
            {
                registry.register(Arc::new(cron::EndRoutineTool::new(
                    self.store.clone(),
                    user.id(),
                    routine,
                )));
            }
        }
        if let Some(search) = &self.tools.search {
            registry.register(search.clone());
        }
        if let Some(key) = &self.tools.firecrawl_key {
            register_web_tools(&mut registry, key.expose_secret());
        }
        if let Some(decision) = &self.tools.decision {
            registry.register(decision.clone());
        }
        if let Some((row, _)) = &research {
            research::register_lead(&mut registry, self, user, row.id);
        }
        if let Some(transitous) = &self.tools.transitous {
            registry.register(Arc::new(tools::GetRouteTool {
                user: user.clone(),
                transitous: transitous.clone(),
                chat,
            }));
        }
        // A turn woken by account events reads text anyone can send, so it
        // only reads: no machines, and below no app tool that acts.
        let untrusted = message
            .woken
            .as_ref()
            .is_some_and(|woken| woken.rule.untrusted());
        // The user's machines: exec, process, desktop and code_agent, and
        // their environment, where commands start and files are kept.
        // Nor does it set up or change routines: a routine's run is trusted,
        // so one made here would act on what the text asked for.
        if untrusted {
            cron::remove_changing_tools(&mut registry);
        }
        let use_machines = self.allows(Privilege::Nodes, user) && !untrusted;
        let workspace = use_machines.then(|| Workspace::find(self, user)).flatten();
        if use_machines {
            tools::machines::register(&mut registry, self, user, chat);
        }
        if events::enabled(self, user) {
            registry.register(Arc::new(events::tools::EventTopicsTool {
                user: user.clone(),
            }));
            registry.register(Arc::new(events::tools::RecentEventsTool {
                user: user.clone(),
            }));
        }
        if message
            .woken
            .as_ref()
            .is_some_and(|woken| woken.rule == wake::Rule::Accounts)
        {
            registry.register(Arc::new(events::tools::HoldEventsTool {
                user: user.clone(),
            }));
        }
        registry.register(Arc::new(tools::SendFileTool {
            user: user.clone(),
            session_id: chat,
            workspace: workspace.clone(),
            machines: use_machines.then(|| tools::machines::Machines {
                app: self.clone(),
                user: user.clone(),
            }),
            paper: research.is_some(),
        }));
        registry.register(Arc::new(tools::SendImageTool {
            user: user.clone(),
            session_id: chat,
            workspace: workspace.clone(),
        }));
        let notify = Arc::new(AtomicBool::new(message.always_sent));
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
        // A routine's run may act without asking where a grant of the
        // routine lets the call through.
        let routine_grants = message
            .cron_id
            .as_deref()
            .filter(|_| is_cron && answer.is_none())
            .and_then(|id| id.parse::<Uuid>().ok())
            .map(|routine| Arc::new(RoutineGrants::new(user.clone(), routine, &run.id, chat)));

        let loading = async { tokio::join!(user.app_tools(), user.mcp_tools(), user.apps()) };
        let Some((composio, mcp, apps)) = cancel.run_until_cancelled(loading).await else {
            return self.stopped_early(user, run, live).await;
        };
        let rules = user.app_rules().await.unwrap_or_else(|error| {
            log::warn!("permissions: unable to load, asking for everything: {error:#}");
            Rules::default()
        });
        let app_tools: Vec<_> = composio.tools.into_iter().chain(mcp).collect();
        let app_tools = if untrusted {
            permissions::read_tools_only(app_tools, &composio.hinted)
        } else {
            app_tools
        };
        let tools = markdown::wrap(app_tools);
        let tools = match &workspace {
            Some(workspace) => files::wrap(tools, workspace),
            None => tools,
        };
        // Grants are asked for where routines are set up. A routine's own
        // run and a turn on untrusted text never ask for one.
        if !is_cron && !untrusted && registry.contains("cron_create") {
            registry.register(Arc::new(RequestGrantTool {
                user: user.clone(),
                approved: approved.0.clone(),
            }));
        }
        for tool in permissions::guard(
            tools,
            rules,
            approved,
            &composio.hinted,
            routine_grants.clone(),
        ) {
            registry.register(tool);
        }
        let refusals = repeat::guard_tool_calls(&mut registry);
        let kind = if research.is_some() {
            "research"
        } else if is_cron {
            "routine"
        } else if background {
            "background"
        } else {
            "coordinator"
        };
        tool_cost::log(kind, &registry);
        let can_map = gates::MAP_TOOLS.iter().any(|tool| registry.contains(tool));
        let can_schedule = gates::SCHEDULE_TOOLS
            .iter()
            .any(|tool| registry.contains(tool));

        // Entries from here on belong to this turn. Matching by time rather than
        // index survives the agent compacting mid-run.
        let turn_started = Utc::now();
        // Conversation entries do not record whether a tool failed; the apps need it.
        let mut failed_tools = HashSet::new();
        // The guarded call the run stops on to ask the user, if any.
        let mut parked = None;
        // The turn's calls, which the gates read with the reply. An approved
        // call that ran on resuming counts, so "sent" after Allow holds.
        let mut calls = Vec::new();
        // The web pages the reply used, shown under it.
        let mut sources = ReplySources::default();

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
                if result.output() != permissions::DENIED {
                    calls.push(gates::Call {
                        name: call.name.clone(),
                        success: result.is_success(),
                    });
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
        let apps = apps.unwrap_or_default();
        let building_context = self.build_context(user, &conversation, message, &apps);
        let Some(mut context) = cancel.run_until_cancelled(building_context).await else {
            return self.stopped_early(user, run, live).await;
        };
        if let Some((row, config)) = &research {
            let block = self.research_context(row, config).await?;
            context = Some(match context {
                Some(context) => format!("{context}\n{block}"),
                None => block,
            });
        }
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
        let mut parts = if sees_images {
            images::labelled(&attached)
        } else {
            let mut descriptions = match describing.take() {
                Some(task) => {
                    let abort = task.abort_handle();
                    let Some(described) = cancel.run_until_cancelled(task).await else {
                        abort.abort();
                        return self.stopped_early(user, run, live).await;
                    };
                    described.unwrap_or_default()
                }
                None => Vec::new(),
            };
            descriptions.resize(attached.len(), None);
            images::described(&descriptions)
        };
        if !message.files.is_empty() {
            let handed: Vec<Handed> = message
                .files
                .iter()
                .map(|file| Handed {
                    name: &file.name,
                    mime: &file.mime,
                    data: &file.data,
                })
                .collect();
            parts.extend(attachments::parts(workspace.as_deref(), &handed).await);
        }

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
            // A finished reply is checked once. A reply that stopped partway
            // through the task is checked again after each pass, up to
            // `MAX_CONTINUES` times.
            let mut checks_left = 1;
            let mut continues = 0;
            // Where the reply stood at the last check, so the next one reads
            // only what the agent wrote since.
            let mut checked_to = 0;
            // When the second pass only adds a card, its text stays off the
            // phone and out of the transcript; this is when that pass began.
            let mut quiet_since = None;
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
                    // Text after a tool call starts a new paragraph; the phone
                    // joins the deltas, so "month." and "Here's" would touch.
                    let mut after_tool = false;
                    // A model repeating itself never stops on its own; end the run instead
                    // of streaming the same sentence until the deadline.
                    let mut text_guard = RepeatGuard::default();
                    let mut reasoning_guard = RepeatGuard::default();
                    loop {
                        let event = tokio::select! {
                            biased;
                            _ = cancel.cancelled() => break self.stopped(),
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
                                if quiet_since.is_some() {
                                    continue;
                                }
                                timer.output(Output::Text);
                                let delta = if mem::take(&mut after_tool)
                                    && !reply.is_empty()
                                    && !reply.ends_with(char::is_whitespace)
                                {
                                    format!("\n\n{delta}")
                                } else {
                                    delta
                                };
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
                                after_tool = true;
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
                                sources: found,
                                ..
                            }) => {
                                tools_running = tools_running.saturating_sub(1);
                                sources.add(&name, &found);
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
                // A failed check gets the agent one more pass to fix it.
                if let Some(since) = quiet_since {
                    drop_text_since(&mut conversation, since);
                }
                if checks_left == 0 || status != RunStatus::Completed || parked.is_some() {
                    break status;
                }
                checks_left -= 1;
                let Some(decision) = &self.tools.decision else {
                    break status;
                };
                let scheduled = self.scheduled_tasks(user, message.cron_id.as_deref()).await;
                let draft = gates::Draft {
                    asked: &message.message,
                    reply: &reply[checked_to..],
                    calls: &calls,
                    scheduled: &scheduled,
                    can_map,
                    can_schedule,
                };
                let verdict = gates::review(decision, &draft).await;
                checked_to = reply.len();
                let Some(verdict) = verdict else {
                    break status;
                };
                if verdict.card_only() {
                    quiet_since = Some(Utc::now());
                }
                if verdict.unfinished() && continues < MAX_CONTINUES {
                    continues += 1;
                    checks_left = 1;
                }
                conversation.add(ConversationEntry::system_message(verdict.note));
            }
        };

        timer.streamed();
        if parked.is_some() && status == RunStatus::Completed {
            status = RunStatus::Waiting;
        }

        // What grants let the run do without asking. Each use already sent
        // its own notification; the chat keeps it as a message.
        let used = routine_grants
            .as_ref()
            .map(|grants| grants.take_used())
            .unwrap_or_default();
        // A routine that found nothing to say and did nothing leaves no
        // trace in the chat.
        let quiet = is_cron && status == RunStatus::Completed && !notify.load(Ordering::SeqCst);
        let silent = quiet && used.is_empty();
        // A routine that told the user something shows only what it told
        // them, which is also the notice: not a sign-off written after it.
        let notice_only = is_cron && status == RunStatus::Completed;
        // The pages go out before the run finishes, so the app that streamed
        // it shows them under the reply at once. A routine shows only its
        // notice, so it has none.
        let sources = (!notice_only && !sources.is_empty()).then(|| sources.to_wire());
        if let Some(sources) = &sources {
            live.push(RunEvent::Sources {
                sources: sources.clone(),
            });
        }
        let told = notice_only
            .then(|| steps::routine_reply(conversation.entries(), Some(turn_started)))
            .flatten();
        let told = match &told {
            Some(Step {
                body: StepBody::Assistant { content },
                ..
            }) => content.as_str(),
            _ if notice_only => "",
            _ => reply.as_str(),
        };
        // A run that stops to ask notifies with the question instead.
        if (is_cron || message.finished_research) && status != RunStatus::Waiting && !quiet {
            self.notify_run_complete(user, run, status, told).await;
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
        // Whether the user corrected how the last turn went about its task,
        // which is worth a skill review now.
        let mut corrected = false;
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
            let follow_up = followups::Input::new(
                &conversation,
                turn_from,
                &exchange.earlier,
                &exchange.latest,
            );
            let kind = followups::Kind::of(&exchange.latest.reply);
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
            let scores = self
                .score_turn(&TurnState {
                    known: &known,
                    exchange: &exchange,
                    follow_up: &follow_up,
                    kind,
                })
                .await;
            if let Some(probability) = scores.memory {
                live.push(RunEvent::MemoryScore { probability });
            }
            let follow_up = follow_up.to_string();
            let replied = exchange.latest.reply.clone();
            // Follow-ups are written only with Jev set up.
            let follow_up = async {
                self.decision()?;
                followups::write(generator, follow_up, kind, &replied, scores.follow_up).await
            };
            // Without a score (Jev not configured or failing) every turn is read,
            // and so is one that acted, since it may finish what a memory plans.
            let acted = !exchange.latest.actions.is_empty();
            let learning = async {
                let learned = match scores.memory {
                    Some(probability) if probability < MEMORY_THRESHOLD && !acted => Vec::new(),
                    _ => user.learn(generator, exchange, &known).await,
                };
                if let Some(probability) = scores.memory {
                    log::info!(
                        "memory score {probability:.2}, {} change(s); run_id={}",
                        learned.len(),
                        run.id
                    );
                }
                learned
            };
            let (learned, follow_up) = tokio::join!(learning, follow_up);
            memory_score = scores.memory;
            corrected = scores.corrected;
            let mut calls: Vec<ServerCall> = learned
                .iter()
                .map(|change| ServerCall {
                    name: tools::MEMORY.to_owned(),
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
        // A stopped turn keeps its pictures rather than wait for their descriptions.
        let stopped = matches!(status, RunStatus::Cancelled | RunStatus::Interrupted);
        if let Some(task) = describing.take_if(|_| stopped) {
            task.abort();
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
        // What the turn ran with sits on its opening message; a resumed turn
        // adds to the one it carries on.
        let record = TurnRecord {
            model: model.to_string(),
            auto_thinking: auto.as_ref().map(|auto| auto.thinking),
            reasoning: String::new(),
            memory_score: if answer.is_none() { memory_score } else { None },
        };
        let mut turns = TurnsByTime::new();
        let calls = steps::CallNotes {
            failed: failed_tools,
            descriptions: live.tool_descriptions(),
        };
        if let Some(asked) = conversation
            .entries()
            .iter()
            .find(|entry| entry.timestamp >= turn_started && entry.is_user())
        {
            turns.insert(asked.timestamp.timestamp_millis(), record);
        }
        // A routine that found nothing to say shows nothing. One that told
        // the user something shows only what it told them, as a message of
        // its own, with the pictures and files it sent: no routine turn, and
        // nothing of how it checked.
        let mut saved = if quiet {
            steps::Steps::default()
        } else if notice_only {
            steps::routine_notice(
                conversation.entries(),
                Some(turn_started),
                steps::steps(conversation.entries(), Some(turn_started), &calls, &turns),
            )
        } else {
            steps::steps(conversation.entries(), Some(turn_started), &calls, &turns)
        };
        // What grants let through comes first, so the reply after it reads
        // in order.
        saved.steps.splice(
            0..0,
            used.into_iter().map(|used| Step {
                sent_at: used.at,
                body: StepBody::Assistant { content: used.text },
            }),
        );
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
        // A routine's turn joins the chat's context, unless it stayed silent.
        let stored = chat_context.map(|mut chat_context| {
            if !silent {
                // Showing the turn drops the chat's waiting card, so the
                // call it asked about is answered as not done rather than
                // left open, which the model would never hear of.
                permissions::close_open_calls(&mut chat_context, permissions::SUPERSEDED);
                conversation
                    .entries()
                    .iter()
                    .filter(|entry| entry.timestamp >= turn_started)
                    .for_each(|entry| chat_context.add(entry.clone()));
            }
            chat_context
        });
        let turn = FinishedTurn {
            context: stored.as_ref().unwrap_or(&conversation),
            prompt_tokens: stored.is_none().then_some(prompt_tokens),
            steps: &saved.steps,
            stands_alone: notice_only,
            carried: TurnRecord {
                reasoning: saved.carried_reasoning.clone(),
                memory_score: if answer.is_some() { memory_score } else { None },
                ..TurnRecord::default()
            },
        };
        let finished = user.finish(run, Some(turn), status).await?;
        // The pictures stay with the message they came with, so every device
        // shows them after the model only keeps their descriptions.
        let opened = finished.and_then(|saved| saved.opened);
        if let Some(turn) = finished.and_then(|saved| saved.turn)
            && let Some(sources) = &sources
        {
            match user.save_sources(chat, turn, sources).await {
                Ok(()) => user.changed(&run.session_id),
                Err(error) => log::warn!(
                    "unable to keep the reply's sources: {error:#}; run_id={}",
                    run.id
                ),
            }
        }
        if let Some(opened) = opened
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
        // A held reply's recording is not kept: spoken turns are text.
        if let Some(opened) = opened
            && message.is_spoken()
        {
            let of = PicturesOf {
                chat,
                message: opened,
            };
            match user.mark_spoken(of).await {
                Ok(()) => user.changed(&run.session_id),
                Err(error) => log::warn!(
                    "unable to mark the turn spoken: {error:#}; run_id={}",
                    run.id
                ),
            }
        }
        if let Some(opened) = opened
            && !message.files.is_empty()
        {
            let of = PicturesOf {
                chat,
                message: opened,
            };
            match user.save_attachments(of, &message.files).await {
                Ok(()) => user.changed(&run.session_id),
                Err(error) => log::warn!(
                    "unable to keep the turn's files: {error:#}; run_id={}",
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
            self.count_for_skills(user, chat, calls, corrected);
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
        // A chat made for a routine's run that found nothing is not kept,
        // so the user never sees an empty one. Another run may have started
        // in it since this one was saved, so it goes only while still empty.
        if silent
            && fresh_chat
            && let Err(error) = user.delete_empty_session(chat).await
        {
            log::warn!(
                "unable to delete a quiet routine's chat: {error:#}; run_id={}",
                run.id
            );
        }
        // A task that ended, such as a routine run, an event's wake or work
        // handed back, is read by the watchers now rather than once quiet.
        if background && !matches!(status, RunStatus::Waiting | RunStatus::Interrupted) {
            self.watchers.task_ended(self, user.id(), chat);
        }
        if let Some(woken) = &message.woken {
            let finished = wake::Finished {
                label: &woken.label,
                status,
                notified: notify.load(Ordering::SeqCst),
                reply: &reply,
            };
            wake::finish(self, user, run, finished).await;
        }
        Ok(())
    }

    /// Ends a run stopped before the model started. Nothing of the turn is
    /// saved, as when it is stopped before the model said anything.
    async fn stopped_early(&self, user: &User, run: &Run, live: &LiveRun) -> Result<()> {
        let status = self.stopped();
        user.finish(run, None, status).await?;
        live.push(RunEvent::RunFinished {
            status: status.as_str().to_owned(),
            updated_at: None,
        });
        Ok(())
    }

    /// The tasks `user` has scheduled, other than the routine running now,
    /// one line each.
    async fn scheduled_tasks(&self, user: &User, running: Option<&str>) -> Vec<String> {
        let jobs = match self.store.cron_jobs(Some(user.id())).await {
            Ok(jobs) => jobs,
            Err(error) => {
                log::warn!("gates: unable to list scheduled tasks: {error:#}");
                return Vec::new();
            }
        };
        jobs.into_iter()
            .filter(|job| job.enabled && running != Some(job.id.to_string().as_str()))
            .filter_map(|job| {
                let line = job.task.lines().find(|line| !line.trim().is_empty())?;
                Some(line.trim().chars().take(SCHEDULED_TASK_CHARS).collect())
            })
            .collect()
    }

    /// How a run ends when it is told to stop: shutting down interrupts it.
    fn stopped(&self) -> RunStatus {
        if self.shutting_down.load(Ordering::Relaxed) {
            RunStatus::Interrupted
        } else {
            RunStatus::Cancelled
        }
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

/// Which languages the user speaks, from their memory, and which of them a
/// reply is in.
fn languages_line(languages: &str) -> String {
    format!(
        "Languages: {languages} Reply in the language of the user's message. When it mixes \
         languages, or a stray word or a misspelling makes it look like one they do not \
         speak, reply in English if they speak it, else in the first language they speak."
    )
}

#[cfg(test)]
mod tests;
