//! One OpenRouter session per model for the whole server, so calls share the provider's prompt cache.

use async_trait::async_trait;
use flashmind::types::{
    CompletionRequest, CompletionStream, ImageGenRequest, LlmProvider, Model, ModelCapabilities,
    ModelInfo, Provider, SessionId, SttRequest, TtsRequest, VideoGenRequest, Voice,
};
use iroh::EndpointId;
use std::{collections::HashMap, sync::Arc};
use uuid::Uuid;

/// A provider that sends every completion with its model's session id.
pub struct SharedCacheSessions {
    inner: Arc<dyn LlmProvider>,
    /// Namespace for the per-model ids, from the server's key.
    server: Uuid,
}

impl SharedCacheSessions {
    pub fn new(inner: Arc<dyn LlmProvider>, server: &EndpointId) -> Self {
        Self {
            inner,
            server: Uuid::new_v5(&Uuid::NAMESPACE_OID, server.as_bytes()),
        }
    }

    /// The session every call to `model` on this server shares.
    fn session(&self, model: &Model) -> SessionId {
        Uuid::new_v5(&self.server, model.name().as_bytes()).into()
    }
}

#[async_trait]
impl LlmProvider for SharedCacheSessions {
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
        self.inner.list_models().await
    }

    fn complete(&self, mut request: CompletionRequest) -> CompletionStream {
        // Set here rather than by each caller, so it follows the model the
        // request is finally sent with.
        request.session_id = Some(self.session(&request.model));
        self.inner.complete(request)
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
    use flashmind::types::{FinishReason, Message, StreamEvent};
    use futures::StreamExt;
    use iroh::SecretKey;

    /// Answers with the session id it was sent.
    struct EchoSession;

    #[async_trait]
    impl LlmProvider for EchoSession {
        fn name(&self) -> &str {
            "test"
        }
        fn provider(&self) -> Provider {
            Provider::OpenRouter
        }
        fn complete(&self, request: CompletionRequest) -> CompletionStream {
            let session = request.session_id.map(String::from).unwrap_or_default();
            Box::pin(async_stream::stream! {
                yield Ok(StreamEvent::ContentDelta(session));
                yield Ok(StreamEvent::Finished(FinishReason::Stop));
            })
        }
    }

    async fn session_sent(
        provider: &SharedCacheSessions,
        model: &str,
        chat: Option<&str>,
    ) -> String {
        let mut request = CompletionRequest::new(
            model.parse().expect("the test model name parses"),
            vec![Message::user("hi")],
        );
        request.session_id = chat.map(|chat| chat.parse().expect("a valid session id"));
        let mut stream = provider.complete(request);
        match stream.next().await {
            Some(Ok(StreamEvent::ContentDelta(session))) => session,
            other => panic!("expected the session id, got {other:?}"),
        }
    }

    fn server(seed: u8) -> EndpointId {
        SecretKey::from_bytes(&[seed; 32]).public()
    }

    #[tokio::test]
    async fn every_call_to_a_model_shares_one_session_per_server() {
        let haiku = "openrouter:anthropic/claude-haiku-5.5";
        let deepseek = "openrouter:deepseek/deepseek-v4";
        let provider = SharedCacheSessions::new(Arc::new(EchoSession), &server(1));

        let first = session_sent(&provider, haiku, Some("chat-a")).await;
        assert_eq!(session_sent(&provider, haiku, Some("chat-b")).await, first);
        assert_eq!(session_sent(&provider, haiku, None).await, first);
        assert_ne!(session_sent(&provider, deepseek, None).await, first);

        let restarted = SharedCacheSessions::new(Arc::new(EchoSession), &server(1));
        assert_eq!(session_sent(&restarted, haiku, None).await, first);
        let other = SharedCacheSessions::new(Arc::new(EchoSession), &server(2));
        assert_ne!(session_sent(&other, haiku, None).await, first);
    }
}
