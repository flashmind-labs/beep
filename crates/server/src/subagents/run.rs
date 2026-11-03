//! Runs one sub-agent until it decides, runs out of turns, tokens or time, or stops.

use super::{
    Kind, Spec, Subagents,
    decision::{self, DecisionTool, Slot},
};
use crate::{
    AsyncMutex,
    repeat::{self, Refusals},
};
use flashmind::{
    core::{AgentHandle, AgentManager, AgentStatus, CancellationToken, SpawnBuilder},
    types::{AgentEvent, ToolRegistry},
};
use std::sync::{Arc, Mutex};
use tokio::{sync::mpsc, time::sleep};

/// Why a run ended without a decision.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Failure {
    #[error("ran out of time")]
    TimedOut,
    #[error("used {used} tokens, over its budget of {budget}")]
    OverBudget { used: u64, budget: u64 },
    #[error("ended without a decision")]
    NoDecision,
    #[error("kept repeating a call")]
    Repeating,
    #[error("was cancelled")]
    Cancelled,
    /// The model or the agent loop failed, including hitting the turn limit.
    #[error("failed: {0}")]
    Failed(String),
}

/// A way for the caller to stop a run.
#[derive(Clone, Default)]
pub struct Watch {
    pub cancel: CancellationToken,
}

/// How a run that did not fail ended.
#[derive(Debug, PartialEq)]
pub enum Ended<D> {
    Decided(D),
    /// It stopped with this reply and no decision.
    Replied(String),
}

/// A running sub-agent's conversation; its manager owns the agent loop.
struct Session<D> {
    /// Held so the agent loop lives as long as the session.
    _manager: AgentManager,
    handle: Arc<AgentHandle>,
    events: AsyncMutex<mpsc::UnboundedReceiver<AgentEvent>>,
    slot: Slot<D>,
    names: Vec<&'static str>,
    refusals: Refusals,
    kind: &'static Kind,
    user: String,
}

impl<D> Session<D> {
    /// Stops what it is doing.
    fn cancel(&self) {
        self.handle.cancel();
    }
}

impl Subagents {
    /// Runs `spec` and returns its decision, failing with [`Failure::NoDecision`] without one.
    pub async fn run<D: Send + 'static>(&self, user: &str, spec: Spec<D>) -> Result<D, Failure> {
        match self.run_or_reply(user, spec).await? {
            Ended::Decided(decided) => Ok(decided),
            Ended::Replied(_) => Err(Failure::NoDecision),
        }
    }

    /// Runs `spec` and returns its decision, or its last reply.
    pub async fn run_or_reply<D: Send + 'static>(
        &self,
        user: &str,
        spec: Spec<D>,
    ) -> Result<Ended<D>, Failure> {
        let watch = spec.watch.clone().unwrap_or_default();
        let session = self.open(user, spec).await?;
        let ended = self.follow(&session, &watch).await;
        session.cancel();
        ended
    }

    /// Starts `spec` and keeps its conversation; [`Self::follow`] watches it.
    async fn open<D: Send + 'static>(
        &self,
        user: &str,
        spec: Spec<D>,
    ) -> Result<Session<D>, Failure> {
        let Spec {
            kind,
            model,
            routing,
            tools,
            decisions,
            task,
            context,
            watch: _,
            gate,
        } = spec;
        let slot: Slot<D> = Arc::new(Mutex::new(None));
        let mut registry = ToolRegistry::new();
        for tool in tools {
            registry.register(tool);
        }
        let names: Vec<&'static str> = decisions.iter().map(|d| d.name).collect();
        for decision in decisions {
            registry.register(Arc::new(DecisionTool {
                decision,
                slot: slot.clone(),
            }));
        }
        let refusals = repeat::guard_tool_calls(&mut registry);
        crate::tool_cost::log(kind.name, &registry);
        if let Some(gate) = gate {
            registry.set_gate(gate);
        }

        let mut llm = self
            .llm
            .with_model(model.model.clone())
            .with_user(user)
            .with_reasoning(model.thinking);
        llm.provider_preferences = routing;
        let task = match context {
            Some(context) => format!("{context}\n\n{task}"),
            None => task,
        };
        let (events, received) = mpsc::unbounded_channel();
        let manager = AgentManager::new(1, 1).with_event_sender(events);
        let spawned = manager
            .spawn(
                SpawnBuilder::new(task, self.provider.clone())
                    .system_prompt(kind.instructions)
                    .llm(llm)
                    .tools(registry)
                    .max_iterations(kind.max_turns),
            )
            .await;
        let handle = match spawned.and_then(|id| manager.conversation(id)) {
            Ok(handle) => handle,
            Err(error) => return Err(Failure::Failed(format!("{error:#}"))),
        };
        Ok(Session {
            _manager: manager,
            handle,
            events: AsyncMutex::new(received),
            slot,
            names,
            refusals,
            kind,
            user: user.to_owned(),
        })
    }

    /// Watches `session` until idle: its latest decision, or its last reply. Limits count from this call.
    async fn follow<D: Send + 'static>(
        &self,
        session: &Session<D>,
        watch: &Watch,
    ) -> Result<Ended<D>, Failure> {
        let Session {
            handle,
            events,
            slot,
            names,
            refusals,
            kind,
            user,
            ..
        } = session;
        let mut received = events.lock().await;
        let mut decided = None;
        let mut used: u64 = 0;
        let deadline = sleep(kind.time_limit);
        tokio::pin!(deadline);
        let ended = loop {
            // Biased so the events a run sent before it ended are read
            // before its end: a run over budget fails even if it finished.
            tokio::select! {
                biased;
                _ = &mut deadline => break Err(Failure::TimedOut),
                _ = self.stop.cancelled() => break Err(Failure::Cancelled),
                _ = watch.cancel.cancelled() => break Err(Failure::Cancelled),
                event = received.recv() => {
                    let Some(AgentEvent::SpawnedEvent { event, .. }) = event else { continue };
                    match *event {
                        AgentEvent::Usage(usage) => {
                            // Summed per report; a provider that reports more
                            // than once per call only makes the guard stricter.
                            used += u64::from(usage.prompt_tokens)
                                + u64::from(usage.completion_tokens);
                            if used > kind.max_tokens {
                                break Err(Failure::OverBudget { used, budget: kind.max_tokens });
                            }
                        }
                        AgentEvent::ToolResult { name, .. } => {
                            if names.contains(&name.as_str())
                                && let Some(decision) = decision::take(slot)
                            {
                                // Done: no model call after the decision.
                                decided = Some(decision);
                                handle.cancel();
                                continue;
                            }
                            if refusals.exhausted() {
                                break Err(Failure::Repeating);
                            }
                        }
                        _ => {}
                    }
                }
                done = handle.wait() => break match done {
                    Ok(reply) => Ok(Some(reply)),
                    Err(_) if decided.is_some() => Ok(None),
                    Err(_) => match handle.status() {
                        AgentStatus::Failed { error } => Err(Failure::Failed(error)),
                        _ => Err(Failure::Cancelled),
                    },
                },
            }
        };
        if ended.is_err() {
            handle.cancel();
        }
        // A decision stands even when the run went on past it.
        match (decided.or_else(|| decision::take(slot)), ended) {
            (Some(decided), _) => Ok(Ended::Decided(decided)),
            (None, Ok(Some(reply))) => Ok(Ended::Replied(reply)),
            (None, Ok(None)) => Err(Failure::NoDecision),
            (None, Err(failure)) => {
                log::info!("subagents: {} ended: {failure}; user={user}", kind.name);
                Err(failure)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        research::RoleModel,
        subagents::{Decision, Kind},
    };
    use async_trait::async_trait;
    use flashmind::types::{
        AgentLlmConfig, CompletionRequest, CompletionStream, FinishReason, LlmProvider, Provider,
        ReasoningLevel, Role, StreamEvent, TokenUsage,
        tool::{Tool, ToolContext, ToolResult},
    };
    use serde_json::{Value, json};
    use std::time::Duration;
    use uuid::Uuid;

    const KIND: Kind = Kind {
        name: "test",
        instructions: "You pick a colour.",
        max_turns: 3,
        max_tokens: 1000,
        time_limit: Duration::from_secs(10),
    };

    /// How the fake model behaves.
    #[derive(Clone, Copy)]
    enum Script {
        /// Calls `pick` with red.
        Decides,
        /// Calls `pick` with a colour it does not know, then red.
        RetriesBadArguments,
        /// Reports more tokens than the budget.
        Expensive,
        /// Calls a lookup tool forever.
        Loops,
    }

    struct Fake(Script);

    #[async_trait]
    impl LlmProvider for Fake {
        fn name(&self) -> &str {
            "test"
        }
        fn provider(&self) -> Provider {
            Provider::OpenAi
        }
        fn complete(&self, request: CompletionRequest) -> CompletionStream {
            let tool_results = request
                .messages
                .iter()
                .filter(|message| message.role == Role::Tool)
                .count();
            match self.0 {
                Script::Decides => call("pick", json!({"colour": "red"}), 10),
                Script::RetriesBadArguments if tool_results == 0 => {
                    call("pick", json!({"colour": "plaid"}), 10)
                }
                Script::RetriesBadArguments => call("pick", json!({"colour": "red"}), 10),
                Script::Expensive => call("lookup", json!({"n": tool_results}), 5000),
                Script::Loops => call("lookup", json!({"n": tool_results}), 10),
            }
        }
    }

    fn usage(tokens: u32) -> StreamEvent {
        StreamEvent::Usage(TokenUsage {
            prompt_tokens: tokens,
            completion_tokens: 0,
            total_tokens: tokens,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
        })
    }

    fn call(name: &str, arguments: Value, tokens: u32) -> CompletionStream {
        let name = name.to_owned();
        Box::pin(async_stream::stream! {
            yield Ok(StreamEvent::ToolCallStart { index: 0, id: Uuid::new_v4().to_string(), name });
            yield Ok(StreamEvent::ToolCallDelta { index: 0, arguments: arguments.to_string() });
            yield Ok(usage(tokens));
            yield Ok(StreamEvent::Finished(FinishReason::ToolCalls));
        })
    }

    #[derive(Debug, PartialEq)]
    struct Colour(String);

    fn pick() -> Decision<Colour> {
        Decision {
            name: "pick",
            description: "Pick a colour.",
            parameters: || json!({"type": "object", "properties": {"colour": {"type": "string"}}}),
            parse: |args| match args["colour"].as_str() {
                Some(colour @ ("red" | "blue")) => Ok(Colour(colour.to_owned())),
                _ => Err("colour must be red or blue".to_owned()),
            },
        }
    }

    /// A read tool that always answers.
    struct Lookup;

    #[async_trait]
    impl Tool for Lookup {
        fn name(&self) -> &str {
            "lookup"
        }
        fn description(&self) -> &str {
            "Look something up."
        }
        fn parameters(&self) -> Value {
            json!({"type": "object", "properties": {"n": {"type": "integer"}}})
        }
        async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
            Ok(ToolResult::success(ctx.tool_call_id, "nothing"))
        }
    }

    fn subagents(script: Script) -> Subagents {
        let model = "openai:test".parse().expect("a model id");
        let (notifications, _) = tokio::sync::broadcast::channel(4);
        Subagents::new(
            Arc::new(Fake(script)),
            AgentLlmConfig::new(model),
            notifications,
        )
    }

    fn spec() -> Spec<Colour> {
        Spec {
            kind: &KIND,
            model: RoleModel {
                model: "openai:test".parse().expect("a model id"),
                thinking: ReasoningLevel::Low,
            },
            routing: None,
            tools: vec![Arc::new(Lookup)],
            decisions: vec![pick()],
            task: "Pick one.".to_owned(),
            context: None,
            watch: None,
            gate: None,
        }
    }

    async fn run(script: Script) -> Result<Colour, Failure> {
        subagents(script).run("alice", spec()).await
    }

    #[tokio::test]
    async fn a_run_returns_the_decision_it_called() {
        assert_eq!(run(Script::Decides).await, Ok(Colour("red".to_owned())));
        assert_eq!(
            run(Script::RetriesBadArguments).await,
            Ok(Colour("red".to_owned()))
        );
    }

    #[tokio::test]
    async fn a_run_stops_at_its_token_budget_and_turn_limit() {
        assert!(matches!(
            run(Script::Expensive).await,
            Err(Failure::OverBudget { budget: 1000, .. })
        ));
        assert!(matches!(
            run(Script::Loops).await,
            Err(Failure::Failed(_) | Failure::Repeating)
        ));
    }

    #[tokio::test]
    async fn a_cancelled_run_stops() {
        let watch = Watch::default();
        watch.cancel.cancel();
        let spec = Spec {
            watch: Some(watch),
            ..spec()
        };
        assert_eq!(
            subagents(Script::Loops).run("alice", spec).await,
            Err(Failure::Cancelled)
        );
    }
}
