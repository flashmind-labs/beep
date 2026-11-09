//! Runs one sub-agent until it decides, runs out of turns, tokens or time, or stops.

use super::{
    Kind, Spec, Subagents,
    decision::{self, DecisionTool, Slot},
};
use crate::{
    AsyncMutex,
    agent::repeat::{self, Refusals},
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
        crate::agent::tool_cost::log(kind.name, &registry);

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
#[path = "../../tests/agent_subagents_run.rs"]
mod tests;
