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
mod tests {
    use super::*;
    use crate::store::TestDb;
    use async_trait::async_trait;
    use flashmind::types::{FinishReason, Message, ModelCategory, ReasoningLevel, SamplingParams};
    use flashmind::types::{ModelCapabilities, Provider};
    use rust_decimal::Decimal;
    use sqlx::Row;
    use std::time::Duration;

    struct UsageStub;

    #[async_trait]
    impl LlmProvider for UsageStub {
        fn name(&self) -> &str {
            "test"
        }
        fn provider(&self) -> Provider {
            Provider::OpenAi
        }
        async fn list_models(&self) -> Option<Vec<ModelInfo>> {
            Some(vec![ModelInfo {
                id: "test".into(),
                name: None,
                context_length: None,
                max_completion_tokens: None,
                capabilities: ModelCapabilities::default(),
                categories: Vec::<ModelCategory>::new(),
                created_at: None,
                pricing: ModelPricing {
                    prompt: Some(Decimal::new(1, 6)),
                    completion: Some(Decimal::new(2, 6)),
                    ..Default::default()
                },
                image: Default::default(),
            }])
        }
        fn complete(&self, request: CompletionRequest) -> CompletionStream {
            // Echoes the user so tests can see what reached the provider.
            let user = request.user.unwrap_or_default();
            Box::pin(async_stream::stream! {
                yield Ok(StreamEvent::ContentDelta(user));
                yield Ok(StreamEvent::Usage(TokenUsage {
                    prompt_tokens: 1000,
                    completion_tokens: 100,
                    total_tokens: 1100,
                    cache_read_tokens: 0,
                    cache_creation_tokens: 0,
                }));
                yield Ok(StreamEvent::Finished(FinishReason::Stop));
            })
        }
    }

    fn request(user: Option<&str>) -> CompletionRequest {
        CompletionRequest {
            model: "openai:test".parse().expect("the test model name parses"),
            messages: vec![Message::user("hi")],
            tools: Vec::new(),
            max_tokens: None,
            reasoning: ReasoningLevel::Off,
            sampling: SamplingParams::default(),
            modalities: Vec::new(),
            audio_config: None,
            image_config: None,
            user: user.map(str::to_owned),
            provider_preferences: None,
            session_id: None,
        }
    }

    #[tokio::test]
    async fn sums_each_users_calls_and_cost() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let metered = Metered::new(Arc::new(UsageStub), store.clone()).await;

        metered.complete(request(Some("alice"))).count().await;
        billed_to("alice", async {
            metered.complete(request(None)).count().await
        })
        .await;
        // Nobody to bill: not saved.
        metered.complete(request(None)).count().await;

        let mut row = None;
        for _ in 0..100 {
            row = sqlx::query("SELECT calls, prompt_tokens, completion_tokens, cost FROM token_usage WHERE user_id='alice'")
                .fetch_optional(store.pool())
                .await
                .unwrap()
                .filter(|row| row.get::<i64, _>("calls") == 2);
            if row.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let row = row.expect("both calls were saved");
        assert_eq!(row.get::<i64, _>("prompt_tokens"), 2000);
        assert_eq!(row.get::<i64, _>("completion_tokens"), 200);
        assert!((row.get::<f64, _>("cost") - 0.0024).abs() < 1e-9);
        let users: i64 = sqlx::query_scalar("SELECT count(DISTINCT user_id) FROM token_usage")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(users, 1);
    }

    #[tokio::test]
    async fn the_billed_user_reaches_the_provider() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let metered = Metered::new(Arc::new(UsageStub), store).await;

        let content = billed_to("alice", async {
            let mut stream = metered.complete(request(None));
            while let Some(event) = stream.next().await {
                if let Ok(StreamEvent::ContentDelta(text)) = event {
                    return text;
                }
            }
            String::new()
        })
        .await;
        assert_eq!(content, "alice");
    }
}
