//! Auto: Jev picks the model and thinking of each of an admin's prompts.

use super::{App, learning::earlier_turns};
use crate::{memories::clip, models::AutoModel, store::TurnRecord, user::User};
use chrono::Utc;
use flashmind::{
    core::Conversation,
    types::{Model, ReasoningLevel},
};
use std::{slice, time::Duration};
use tokio::time::Instant;
use uuid::Uuid;

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
    pub session: Uuid,
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
