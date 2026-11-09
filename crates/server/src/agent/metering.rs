//! Counts every model call's tokens and cost against the user it ran for.

use super::forward::forward_llm;
use crate::store::Store;
use flashmind::types::{
    CompletionRequest, CompletionStream, LlmProvider, ModelInfo, ModelPricing, StreamEvent,
    TokenUsage,
};
use futures::StreamExt;
use rust_decimal::prelude::ToPrimitive;
use std::{cell::Cell, collections::HashMap, future::Future, sync::Arc};

tokio::task_local! {
    /// The user a call is billed to when its request names none.
    static BILLED_TO: String;
    /// Whether the running turn is a research's, once it is known.
    static RESEARCH_TURN: Cell<bool>;
}

/// Runs `work` with its model calls counted for `user`.
pub async fn billed_to<F: Future>(user: &str, work: F) -> F::Output {
    BILLED_TO.scope(user.to_owned(), work).await
}

/// Runs a turn, so calls after `research_turn` count as research.
pub async fn in_turn<F: Future>(work: F) -> F::Output {
    RESEARCH_TURN.scope(Cell::new(false), work).await
}

/// Counts the rest of the running turn's calls as research.
pub fn research_turn() {
    // Outside `in_turn` there is no turn to mark.
    let _ = RESEARCH_TURN.try_with(|turn| turn.set(true));
}

/// One finished call: what it used and what that cost.
pub struct CallUsage {
    pub model: String,
    /// Whether it ran in a research's turn.
    pub research: bool,
    pub tokens: TokenUsage,
    /// USD, zero when the model is unpriced.
    pub cost: f64,
}

/// A provider that saves the usage each completion reports.
pub struct Metered {
    inner: Arc<dyn LlmProvider>,
    store: Store,
    /// Listed once at startup, which also serves `list_models`.
    models: Option<Vec<ModelInfo>>,
    prices: HashMap<String, ModelPricing>,
}

impl Metered {
    pub async fn new(inner: Arc<dyn LlmProvider>, store: Store) -> Self {
        let models = inner.list_models().await;
        let prices = models
            .iter()
            .flatten()
            .map(|info| (info.id.clone(), info.pricing.clone()))
            .collect();
        Self {
            inner,
            store,
            models,
            prices,
        }
    }
}

forward_llm!(Metered {
    async fn list_models(&self) -> Option<Vec<ModelInfo>> {
        self.models.clone()
    }

    fn complete(&self, mut request: CompletionRequest) -> CompletionStream {
        // Name the billed user on the request too, so the provider's own
        // dashboard (OpenRouter's activity log) can split usage by user.
        if request.user.is_none() {
            request.user = BILLED_TO.try_with(String::clone).ok();
        }

        let Some(user) = request.user.clone() else {
            log::warn!(
                "model call with no user is not counted; model={}",
                request.model
            );
            return self.inner.complete(request);
        };
        let model = request.model.to_string();
        let research = RESEARCH_TURN.try_with(Cell::get).unwrap_or(false);
        let pricing = self.prices.get(request.model.name()).cloned();
        let store = self.store.clone();
        let mut usage: Option<TokenUsage> = None;
        let stream = self
            .inner
            .complete(request)
            .inspect(move |event| match event {
                // Some providers send running totals, so the last one counts.
                Ok(StreamEvent::Usage(tokens)) => usage = Some(tokens.clone()),
                Ok(StreamEvent::Finished(_)) => {
                    let Some(tokens) = usage.take() else {
                        return;
                    };
                    let cost = pricing
                        .as_ref()
                        .and_then(|pricing| tokens.cost(pricing))
                        .and_then(|cost| cost.to_f64())
                        .unwrap_or_default();
                    let call = CallUsage {
                        model: model.clone(),
                        research,
                        tokens,
                        cost,
                    };
                    let store = store.clone();
                    let user = user.clone();
                    tokio::spawn(async move {
                        if let Err(error) = store.add_token_usage(&user, &call).await {
                            log::warn!("unable to save token usage: {error}");
                        }
                    });
                }
                _ => {}
            });
        Box::pin(stream)
    }
});

#[cfg(test)]
#[path = "../tests/agent_metering.rs"]
mod tests;
