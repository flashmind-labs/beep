//! Auto: Jev picks the thinking of every user's prompts, on the model they
//! chose. An admin gets it only with Auto on.
//! Agents: once an admin turns them on, Jev picks the agent of every
//! user's prompts, which brings its own prompt, model and thinking.

use super::{App, learning::earlier_turns};
use crate::{
    agents::{Agent, AgentId, router_state},
    instance::Privilege,
    memories::clip,
    models::AutoModel,
    store::TurnRecord,
    user::User,
};
use chrono::Utc;
use flashmind::{
    core::Conversation,
    types::{Model, ReasoningLevel},
};
use std::{slice, time::Duration};
use tokio::time::{Instant, timeout};
use uuid::Uuid;

/// Characters of the prompt Jev reads.
const MAX_PROMPT_CHARS: usize = 4_000;
/// How long Jev may take to pick an agent before the turn runs on General.
const AGENT_TIMEOUT: Duration = Duration::from_secs(5);

/// What Jev picked for a turn.
pub(super) struct AutoChoice {
    pub model: Model,
    pub thinking: ReasoningLevel,
}

/// The agent a turn runs as, and the thinking Jev picked for it.
pub(super) struct AgentTurn {
    pub agent: Agent,
    pub wanted: ReasoningLevel,
}

impl AgentTurn {
    /// The agent's model, at the thinking Jev picked.
    pub fn choice(&self) -> AutoChoice {
        AutoChoice {
            model: self.agent.model.clone(),
            thinking: self.wanted,
        }
    }
}

/// The prompt Jev routes, with what came before it.
pub(super) struct Prompt<'a> {
    pub message: &'a str,
    pub images: usize,
    pub conversation: &'a Conversation,
    pub session: Uuid,
}

impl App {
    /// Whether an admin turned Auto on, so Jev picks their thinking.
    async fn admin_auto_on(&self, user: &User) -> bool {
        user.auto_model().await.unwrap_or_else(|error| {
            log::warn!("reading auto failed; user={}: {error:#}", user.id());
            false
        })
    }

    /// Jev's pick of the thinking for `prompt`, on the user's own model.
    /// `None` when an admin has Auto off or Jev fails, so the turn runs on
    /// the user's own settings.
    pub(super) async fn auto_pick(&self, user: &User, prompt: &Prompt<'_>) -> Option<AutoChoice> {
        let decision = self.decision()?;
        if self.models.is_admin(user.id()) && !self.admin_auto_on(user).await {
            return None;
        }
        let model = self.model_for(user).await;
        let own = AutoModel {
            name: model.name().to_owned(),
            model,
            when: String::new(),
        };
        let started = Instant::now();
        match decision
            .route(&state(prompt, None), slice::from_ref(&own))
            .await
        {
            Ok(routed) => {
                log::info!(
                    "auto picked; user={} model={} thinking={} ms={}",
                    user.id(),
                    routed.model.model,
                    routed.thinking,
                    started.elapsed().as_millis()
                );
                Some(AutoChoice {
                    model: routed.model.model.clone(),
                    thinking: routed.thinking,
                })
            }
            Err(error) => {
                log::warn!("auto pick failed; user={}: {error:#}", user.id());
                None
            }
        }
    }

    /// Whether agents are set up and an admin turned them on. On a
    /// single-user server they start on, unless its settings turn them off.
    pub(super) async fn agents_on(&self) -> bool {
        if self.decision().is_none() || self.models.agents().is_empty() {
            return false;
        }
        if self
            .instance()
            .is_some_and(|instance| !instance.allows(Privilege::Agents))
        {
            return false;
        }
        let default = self.agents_by_default();
        self.store
            .agents_enabled(default)
            .await
            .unwrap_or_else(|error| {
                log::warn!("reading the agents setting failed: {error:#}");
                false
            })
    }

    /// Jev's agent for `prompt`, told which agent the chat is on. General
    /// when Jev fails or is slow. Call only when `agents_on`.
    pub(super) async fn agent_pick(&self, user: &User, prompt: &Prompt<'_>) -> AgentTurn {
        let agents = self.models.agents();
        let general = AgentTurn {
            agent: agents[0].clone(),
            wanted: agents[0].thinking,
        };
        let Some(decision) = self.decision() else {
            return general;
        };
        let current = last_turn(user, prompt.session)
            .await
            .and_then(|turn| turn.agent)
            .and_then(|id| agents.iter().find(|agent| agent.id == id));
        let started = Instant::now();
        let routed = timeout(
            AGENT_TIMEOUT,
            decision.route_agent(&state(prompt, current), agents),
        )
        .await;
        match routed {
            Ok(Ok(routed)) => {
                log::info!(
                    "agent picked; user={} agent={} kept={} thinking={} ms={}",
                    user.id(),
                    routed.agent.id.as_str(),
                    current.is_some_and(|current| current.id == routed.agent.id),
                    routed.thinking,
                    started.elapsed().as_millis()
                );
                AgentTurn {
                    agent: routed.agent.clone(),
                    wanted: routed.thinking,
                }
            }
            Ok(Err(error)) => {
                log::warn!("agent pick failed; user={}: {error:#}", user.id());
                general
            }
            Err(_) => {
                log::warn!("agent pick timed out; user={}", user.id());
                general
            }
        }
    }

    /// The configured agent `id`, or `None` when the config no longer has it.
    pub(super) fn configured_agent(&self, id: AgentId) -> Option<Agent> {
        self.models
            .agents()
            .iter()
            .find(|agent| agent.id == id)
            .cloned()
    }

    /// The agent the turn a resumed session stopped in ran as.
    pub(super) async fn resumed_agent(&self, user: &User, session: Uuid) -> Option<Agent> {
        let id = last_turn(user, session).await?.agent?;
        self.configured_agent(id)
    }

    /// The model and thinking a turn as `agent` runs on. Everyone but an
    /// admin gets the agent's own; an admin with Auto on keeps their model at
    /// the thinking picked for the agent, and one with Auto off picks both.
    pub(super) async fn agent_choice(&self, user: &User, agent: &AgentTurn) -> Option<AutoChoice> {
        if !self.models.is_admin(user.id()) {
            return Some(agent.choice());
        }
        if !self.admin_auto_on(user).await {
            return None;
        }
        Some(AutoChoice {
            model: self.model_for(user).await,
            thinking: agent.wanted,
        })
    }

    /// What Jev picked for the turn a resumed session stopped in, so the
    /// turn ends on the same model.
    pub(super) async fn resumed_pick(&self, user: &User, session: Uuid) -> Option<AutoChoice> {
        let turn = last_turn(user, session).await?;
        Some(AutoChoice {
            thinking: turn.auto_thinking?,
            model: self.models.find(&turn.model)?.clone(),
        })
    }
}

/// What the newest turn of `session` ran with, when it is known.
async fn last_turn(user: &User, session: Uuid) -> Option<TurnRecord> {
    user.last_turn(session)
        .await
        .inspect_err(|error| log::warn!("reading the last turn failed: {error:#}"))
        .ok()
        .flatten()
}

/// The latest message and the turn before it, as Jev reads them, with the
/// agent the chat is on when there is one.
fn state(prompt: &Prompt<'_>, current: Option<&Agent>) -> String {
    let earlier = earlier_turns(prompt.conversation, Utc::now())
        .pop()
        .map(|turn| turn.to_string());
    router_state(
        current.map(|agent| agent.id),
        earlier.as_deref(),
        &clip(prompt.message, MAX_PROMPT_CHARS),
        prompt.images,
    )
}
