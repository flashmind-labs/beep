//! Counts every model call's tokens and cost against the user it ran for.

use crate::store::Store;
use async_trait::async_trait;
use flashmind::types::{
    CompletionRequest, CompletionStream, ImageGenRequest, LlmProvider, Model, ModelCapabilities,
    ModelInfo, ModelPricing, Provider, StreamEvent, SttRequest, TokenUsage, TtsRequest,
    VideoGenRequest, Voice,
};
use futures::StreamExt;
use rust_decimal::prelude::ToPrimitive;
use std::{collections::HashMap, future::Future, sync::Arc};

tokio::task_local! {
    /// The user a call is billed to when its request names none.
    static BILLED_TO: String;
}

/// Runs `work` with its model calls counted for `user`, for code that builds
/// requests it cannot set a user on, such as flashmind's compaction.
pub async fn billed_to<F: Future>(user: &str, work: F) -> F::Output {
    BILLED_TO.scope(user.to_owned(), work).await
}

/// One finished call: what it used and what that cost.
pub struct CallUsage {
    pub model: String,
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

#[async_trait]
impl LlmProvider for Metered {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn provider(&self) -> Provider {
        self.inner.provider()
    }

    async fn context_window(&self, model: &Model) -> Option<u32> {
        self.inner.context_window(model).await
    }

    async fn capabilities(&self, model: &Model) -> ModelCapabilities {
        self.inner.capabilities(model).await
    }

    async fn list_models(&self) -> Option<Vec<ModelInfo>> {
        self.models.clone()
    }

    fn complete(&self, request: CompletionRequest) -> CompletionStream {
        let user = request
            .user
            .clone()
            .or_else(|| BILLED_TO.try_with(String::clone).ok());
        let Some(user) = user else {
            log::warn!(
                "model call with no user is not counted; model={}",
                request.model
            );
            return self.inner.complete(request);
        };
        let model = request.model.to_string();
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

    fn text_to_speech(&self, request: TtsRequest) -> CompletionStream {
        self.inner.text_to_speech(request)
    }

    fn transcribe(&self, request: SttRequest) -> CompletionStream {
        self.inner.transcribe(request)
    }

    async fn list_voices(&self, model: &str) -> Option<Vec<Voice>> {
        self.inner.list_voices(model).await
    }

    fn generate_image(&self, request: ImageGenRequest) -> CompletionStream {
        self.inner.generate_image(request)
    }

    fn generate_video(&self, request: VideoGenRequest) -> CompletionStream {
        self.inner.generate_video(request)
    }

    fn update_routing(&self, routing: &HashMap<String, String>) {
        self.inner.update_routing(routing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::TestDb;
    use flashmind::types::{FinishReason, Message, ModelCategory, ReasoningLevel, SamplingParams};
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
                pricing: ModelPricing {
                    prompt: Some(Decimal::new(1, 6)),
                    completion: Some(Decimal::new(2, 6)),
                    ..Default::default()
                },
            }])
        }
        fn complete(&self, _request: CompletionRequest) -> CompletionStream {
            Box::pin(async_stream::stream! {
                yield Ok(StreamEvent::ContentDelta("hi".into()));
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
}
