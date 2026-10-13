//! Auto: Jev picks the thinking of every user's prompts, and for an admin
//! with Auto on the model too.
//! Agents: once an admin turns them on, Jev picks the agent of every
//! user's prompts, which brings its own prompt, model and thinking.

use super::{App, learning::earlier_turns};
use crate::{
    agents::{Agent, AgentId, router_state},
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
/// How long after a turn the next one keeps its model, so the provider's
/// prompt cache is still warm.
const CACHE_WINDOW: Duration = Duration::from_secs(5 * 60);
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
    /// Whether Jev picks `user`'s model and thinking: an admin with Auto on,
    /// on a server with Auto models and Jev.
    async fn admin_auto_on(&self, user: &User) -> bool {
        if self.models.auto().is_empty() {
            return false;
        }
        user.auto_model().await.unwrap_or_else(|error| {
            log::warn!("reading auto failed; user={}: {error:#}", user.id());
            false
        })
    }

    /// Jev's pick for `prompt`. Everyone but an admin gets only the thinking,
    /// on their own model. `None` when Auto is off or Jev fails, so the turn
    /// runs on the user's own settings.
    pub(super) async fn auto_pick(&self, user: &User, prompt: &Prompt<'_>) -> Option<AutoChoice> {
        let decision = self.decision()?;
        let own;
        let (models, kept) = if !self.models.is_admin(user.id()) {
            let model = self.model_for(user).await;
            own = AutoModel {
                name: model.name().to_owned(),
                model,
                when: String::new(),
            };
            (slice::from_ref(&own), false)
        } else if self.admin_auto_on(user).await {
            // A recent turn keeps its model, so only the thinking is picked.
            match self.recent_model(user, prompt).await {
                Some(kept) => (slice::from_ref(kept), true),
                None => (self.models.auto(), false),
            }
        } else {
            return None;
        };
        let started = Instant::now();
        match decision.route(&state(prompt, None), models).await {
            Ok(routed) => {
                log::info!(
                    "auto picked; user={} model={} kept={kept} thinking={} ms={}",
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

    /// Whether agents are set up and an admin turned them on.
    pub(super) async fn agents_on(&self) -> bool {
        if self.decision().is_none() || self.models.agents().is_empty() {
            return false;
        }
        self.store.agents_enabled().await.unwrap_or_else(|error| {
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

    /// Whether the agent also picks `user`'s model and thinking: for
    /// everyone but an admin who turned Auto off and picks their own.
    pub(super) async fn agent_picks_model(&self, user: &User) -> bool {
        if !self.models.is_admin(user.id()) {
            return true;
        }
        user.auto_model().await.unwrap_or_else(|error| {
            log::warn!("reading auto failed; user={}: {error:#}", user.id());
            true
        })
    }

    /// The Auto model the turn before `prompt` ran on, when that turn ended
    /// within `CACHE_WINDOW`.
    async fn recent_model(&self, user: &User, prompt: &Prompt<'_>) -> Option<&AutoModel> {
        let entries = prompt.conversation.entries();
        let age = (Utc::now() - entries.last()?.timestamp).to_std().ok()?;
        if age > CACHE_WINDOW {
            return None;
        }
        let turn = last_turn(user, prompt.session).await?;
        self.models
            .auto()
            .iter()
            .find(|auto| auto.model.to_string() == turn.model)
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
