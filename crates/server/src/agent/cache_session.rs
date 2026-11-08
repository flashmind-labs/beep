//! One OpenRouter session per model for the whole server, so calls share the provider's prompt cache.

use super::forward::forward_llm;
use flashmind::types::{
    CompletionRequest, CompletionStream, LlmProvider, Model, ModelInfo, SessionId,
};
use iroh::EndpointId;
use std::sync::Arc;
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

forward_llm!(SharedCacheSessions {
    async fn list_models(&self) -> Option<Vec<ModelInfo>> {
        self.inner.list_models().await
    }

    fn complete(&self, mut request: CompletionRequest) -> CompletionStream {
        // Set here rather than by each caller, so it follows the model the
        // request is finally sent with.
        request.session_id = Some(self.session(&request.model));
        self.inner.complete(request)
    }
});

#[cfg(test)]
#[path = "../tests/agent_cache_session.rs"]
mod tests;
