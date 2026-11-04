//! `LlmProvider` for a wrapper: all but `list_models` and `complete` go to `self.inner`.

macro_rules! forward_llm {
    ($wrapper:ty { $($own:tt)* }) => {
        #[async_trait::async_trait]
        impl flashmind::types::LlmProvider for $wrapper {
            fn name(&self) -> &str {
                self.inner.name()
            }

            fn provider(&self) -> flashmind::types::Provider {
                self.inner.provider()
            }

            async fn context_window(&self, model: &flashmind::types::Model) -> Option<u32> {
                self.inner.context_window(model).await
            }

            async fn capabilities(
                &self,
                model: &flashmind::types::Model,
            ) -> flashmind::types::ModelCapabilities {
                self.inner.capabilities(model).await
            }

            fn text_to_speech(
                &self,
                request: flashmind::types::TtsRequest,
            ) -> flashmind::types::CompletionStream {
                self.inner.text_to_speech(request)
            }

            fn transcribe(
                &self,
                request: flashmind::types::SttRequest,
            ) -> flashmind::types::CompletionStream {
                self.inner.transcribe(request)
            }

            async fn list_voices(&self, model: &str) -> Option<Vec<flashmind::types::Voice>> {
                self.inner.list_voices(model).await
            }

            fn generate_image(
                &self,
                request: flashmind::types::ImageGenRequest,
            ) -> flashmind::types::CompletionStream {
                self.inner.generate_image(request)
            }

            fn generate_video(
                &self,
                request: flashmind::types::VideoGenRequest,
            ) -> flashmind::types::CompletionStream {
                self.inner.generate_video(request)
            }

            fn update_routing(&self, routing: &std::collections::HashMap<String, String>) {
                self.inner.update_routing(routing)
            }

            $($own)*
        }
    };
}

pub(super) use forward_llm;
