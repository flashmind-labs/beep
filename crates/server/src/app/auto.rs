//! Auto: Jev picks the thinking of a user's prompts, on the model they
//! chose. On for everyone until they turn it off.

use super::{App, learning::earlier_turns};
use crate::{memories::clip, models::AutoModel, store::TurnRecord, user::User};
use chrono::Utc;
use flashmind::{
    core::Conversation,
    types::{Model, ReasoningLevel},
};
use std::{fmt::Write, slice};
use tokio::time::Instant;
use uuid::Uuid;

/// Characters of the prompt Jev reads.
const MAX_PROMPT_CHARS: usize = 4_000;

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
}

impl App {
    /// Whether the user has Auto on, so Jev picks their thinking.
    async fn auto_on(&self, user: &User) -> bool {
        user.auto_model().await.unwrap_or_else(|error| {
            log::warn!("reading auto failed; user={}: {error:#}", user.id());
            false
        })
    }

    /// Jev's pick of the thinking for `prompt`, on the user's own model.
    /// `None` when the user has Auto off or Jev fails, so the turn runs on
    /// their own settings.
    pub(super) async fn auto_pick(&self, user: &User, prompt: &Prompt<'_>) -> Option<AutoChoice> {
        let decision = self.decision()?;
        if self.models.auto().is_empty() || !self.auto_on(user).await {
            return None;
        }
        let model = self.model_for(user).await;
        let own = AutoModel {
            name: model.name().to_owned(),
            model,
            when: String::new(),
        };
        let started = Instant::now();
        match decision.route(&state(prompt), slice::from_ref(&own)).await {
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
        let _ = write!(state, "Earlier turn:\n{turn}\n\n");
    }
    state.push_str("Latest message from the user: ");
    state.push_str(&clip(prompt.message, MAX_PROMPT_CHARS));
    if prompt.images > 0 {
        let _ = write!(state, "\n({} image(s) attached)", prompt.images);
    }
    state
}
