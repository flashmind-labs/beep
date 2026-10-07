//! Auto: Jev picks the model and thinking of each of an admin's prompts.

use super::{App, learning::earlier_turns};
use crate::{memories::clip, models::AutoModel, user::User};
use chrono::Utc;
use flashmind::{
    core::Conversation,
    types::{Model, ReasoningLevel},
};
use std::{slice, time::Duration};
use tokio::time::Instant;

/// Characters of the prompt Jev reads.
const MAX_PROMPT_CHARS: usize = 4_000;
/// How long after a turn the next one keeps its model, so the provider's
/// prompt cache is still warm.
const CACHE_WINDOW: Duration = Duration::from_secs(5 * 60);

/// What Jev picked for a turn.
pub(super) struct AutoChoice {
    pub model: Model,
    pub thinking: ReasoningLevel,
}

/// The prompt Jev routes, with what came before it.
pub(super) struct Prompt<'a> {
    pub message: &'a str,
    pub images: usize,
    pub conversation: &'a Conversation,
    pub session: &'a str,
}

impl App {
    /// Whether Jev picks `user`'s model and thinking: an admin with Auto on,
    /// on a server with Auto models and Jev.
    async fn auto_on(&self, user: &User) -> bool {
        if self.decision().is_none()
            || self.models.auto().is_empty()
            || !self.models.is_admin(user.id())
        {
            return false;
        }
        user.auto_model().await.unwrap_or_else(|error| {
            log::warn!("reading auto failed; user={}: {error:#}", user.id());
            false
        })
    }

    /// Jev's pick for `prompt`. `None` when Auto is off or Jev fails, so
    /// the turn runs on the user's own settings.
    pub(super) async fn auto_pick(&self, user: &User, prompt: &Prompt<'_>) -> Option<AutoChoice> {
        if !self.auto_on(user).await {
            return None;
        }
        let decision = self.decision()?;
        // A recent turn keeps its model, so only the thinking is picked.
        let kept = self.recent_model(user, prompt).await;
        let models = kept.map_or(self.models.auto(), slice::from_ref);
        let started = Instant::now();
        match decision.route(&state(prompt), models).await {
            Ok(routed) => {
                log::info!(
                    "auto picked; user={} model={} kept={} thinking={} ms={}",
                    user.id(),
                    routed.model.model,
                    kept.is_some(),
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

    /// The Auto model the turn before `prompt` ran on, when that turn ended
    /// within `CACHE_WINDOW`.
    async fn recent_model(&self, user: &User, prompt: &Prompt<'_>) -> Option<&AutoModel> {
        let entries = prompt.conversation.entries();
        let age = (Utc::now() - entries.last()?.timestamp).to_std().ok()?;
        if age > CACHE_WINDOW {
            return None;
        }
        let asked = entries.iter().rev().find(|e| e.is_user())?;
        let models = user
            .turn_models(prompt.session)
            .await
            .inspect_err(|error| log::warn!("reading turn models failed: {error:#}"))
            .ok()?;
        let turn = models.get(&asked.timestamp.timestamp_millis())?;
        self.models
            .auto()
            .iter()
            .find(|auto| auto.model.to_string() == turn.model)
    }

    /// What Jev picked for the turn a resumed session stopped in, found by
    /// its last user message in `transcript`, so the turn ends on the same
    /// model.
    pub(super) async fn resumed_pick(
        &self,
        user: &User,
        session: &str,
        transcript: &Conversation,
    ) -> Option<AutoChoice> {
        let asked = transcript.entries().iter().rev().find(|e| e.is_user())?;
        let models = user
            .turn_models(session)
            .await
            .inspect_err(|error| log::warn!("reading turn models failed: {error:#}"))
            .ok()?;
        let turn = models.get(&asked.timestamp.timestamp_millis())?;
        Some(AutoChoice {
            thinking: turn.auto_thinking?,
            model: self.models.find(&turn.model)?.clone(),
        })
    }
}

/// The latest message and the turn before it, as Jev reads them.
fn state(prompt: &Prompt<'_>) -> String {
    let mut state = String::new();
    if let Some(turn) = earlier_turns(prompt.conversation, Utc::now()).pop() {
        state.push_str(&format!("Earlier turn:\n{turn}\n\n"));
    }
    state.push_str("Latest message from the user: ");
    state.push_str(&clip(prompt.message, MAX_PROMPT_CHARS));
    if prompt.images > 0 {
        state.push_str(&format!("\n({} image(s) attached)", prompt.images));
    }
    state
}
