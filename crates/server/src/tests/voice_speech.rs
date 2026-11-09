use super::*;
use async_trait::async_trait;
use flashmind::types::{
    AgentLlmConfig, CompletionRequest, CompletionStream, FinishReason, ModelInfo,
};
use std::sync::Mutex as StdMutex;

/// A provider whose rewrites fail, that answers speech and transcripts
/// from fixed text and records what it was asked.
#[derive(Default)]
struct Stub {
    /// The transcript; `None` fails the call.
    transcript: Option<&'static str>,
    asked: StdMutex<Vec<String>>,
}

impl Stub {
    fn asked(&self) -> Vec<String> {
        self.asked.lock().expect("the lock is not poisoned").clone()
    }

    fn note(&self, what: String) {
        self.asked
            .lock()
            .expect("the lock is not poisoned")
            .push(what);
    }
}

#[async_trait]
impl LlmProvider for Stub {
    fn name(&self) -> &str {
        "stub"
    }
    fn provider(&self) -> Provider {
        Provider::OpenRouter
    }
    async fn list_models(&self) -> Option<Vec<ModelInfo>> {
        None
    }
    fn complete(&self, request: CompletionRequest) -> CompletionStream {
        self.note(format!("complete:{}", request.messages.len()));
        Box::pin(async_stream::stream! {
            yield Err(anyhow::anyhow!("API error (500): secret body"));
        })
    }
    fn text_to_speech(&self, request: TtsRequest) -> CompletionStream {
        self.note(format!(
            "speech:{}:{}:{}:{}",
            request.model, request.voice, request.response_format, request.input
        ));
        Box::pin(async_stream::stream! {
            yield Ok(StreamEvent::AudioDelta { data: b"ID3".to_vec(), format: "mp3".into() });
            yield Ok(StreamEvent::AudioDelta { data: vec![0xff, 0xfb], format: "mp3".into() });
            yield Ok(StreamEvent::Finished(FinishReason::Stop));
        })
    }
    fn transcribe(&self, request: SttRequest) -> CompletionStream {
        self.note(format!(
            "transcribe:{}:{}:{}",
            request.model,
            request.media_type,
            request.audio.len()
        ));
        let transcript = self.transcript;
        Box::pin(async_stream::stream! {
            match transcript {
                Some(text) => {
                    yield Ok(StreamEvent::ContentDelta(text.to_owned()));
                    yield Ok(StreamEvent::Finished(FinishReason::Stop));
                }
                None => yield Err(anyhow::anyhow!("STT API error (502 Bad Gateway): secret body")),
            }
        })
    }
}

pub(super) fn wav(data: Vec<u8>) -> Upload {
    Upload {
        name: "reply.wav".into(),
        mime: "audio/x-wav".into(),
        data,
    }
}

pub(super) fn llm() -> AgentLlmConfig {
    AgentLlmConfig::new("openrouter:test/model".parse().expect("a valid model"))
}

#[tokio::test]
pub(super) async fn refuses_bad_audio_before_calling() {
    let stub = Arc::new(Stub::default());
    let voice = Voice::new(stub.clone(), None, VoiceConfig::default());
    let mut text = wav(vec![1; 10]);
    text.mime = "text/plain".into();
    assert!(matches!(
        voice.transcribe("u1", &text).await,
        Err(VoiceError::BadAudio)
    ));
    assert!(matches!(
        voice.transcribe("u1", &wav(Vec::new())).await,
        Err(VoiceError::BadAudio)
    ));
    assert!(matches!(
        voice.transcribe("u1", &wav(vec![0; MAX_VOICE + 1])).await,
        Err(VoiceError::BadAudio)
    ));
    assert!(stub.asked().is_empty());
}

#[tokio::test]
pub(super) async fn a_failed_or_silent_transcription_says_only_what_happened() {
    let voice = Voice::new(Arc::new(Stub::default()), None, VoiceConfig::default());
    let failed = voice.transcribe("u1", &wav(vec![1])).await;
    assert!(matches!(failed, Err(VoiceError::Provider)));
    assert!(
        !failed
            .expect_err("it failed")
            .to_string()
            .contains("secret")
    );
    let silent = Stub {
        transcript: Some(" "),
        ..Stub::default()
    };
    let voice = Voice::new(Arc::new(silent), None, VoiceConfig::default());
    let heard = voice.transcribe("u1", &wav(vec![1])).await;
    assert!(matches!(heard, Err(VoiceError::Silent)));
}

#[tokio::test]
pub(super) async fn a_user_is_limited_but_others_are_not() {
    let voice = Voice::new(Arc::new(Stub::default()), None, VoiceConfig::default());

    for _ in 0..RATE_CALLS {
        voice.speak("u1", "hi").await.expect("within the limit");
    }

    assert!(matches!(
        voice.speak("u1", "hi").await,
        Err(VoiceError::RateLimited)
    ));
    voice.speak("u2", "hi").await.expect("another user");
}

#[tokio::test]
pub(super) async fn a_failed_rewrite_speaks_the_notice_made_plain() {
    let stub = Stub::default();
    let llm = llm();
    let generator = Generator {
        provider: &stub,
        llm: &llm,
        user: "u1",
    };
    let text = spoken_text(
        &generator,
        "Morning briefing",
        "**Rain** until 11:00, see https://weather.example/today",
    )
    .await;
    assert_eq!(text, "Morning briefing. Rain until 11:00, see");
}
