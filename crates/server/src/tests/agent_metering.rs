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
