//! Voice notes: a notice rewritten for listening and spoken, and a spoken
//! reply turned into text, through the provider's speech and transcription
//! endpoints. The key and upstream error bodies never reach clients.

use crate::{
    protocol::{MAX_VOICE, Upload, VOICE_TYPES},
    starters::Generator,
};
use flashmind::types::{
    AliasedModel, AudioFormat, LlmProvider, Model, Provider, StreamEvent, SttRequest, TtsRequest,
};
use futures::StreamExt;
use serde::Deserialize;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};
use thiserror::Error;
use tokio::{sync::Semaphore, time::timeout};

/// Speech and transcription calls the whole server makes at once.
const MAX_CALLS: usize = 8;
/// How long a call waits for a free slot.
const QUEUE_WAIT: Duration = Duration::from_secs(10);
/// How long the provider may take to speak a note or transcribe a reply.
const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the rewrite for listening may take before the plain text is
/// spoken instead.
const REWRITE_TIMEOUT: Duration = Duration::from_secs(12);
/// Calls one user may make within `RATE_WINDOW`.
const RATE_CALLS: usize = 40;
/// Live mode's lines one user may have spoken within `RATE_WINDOW`: a
/// reply is read sentence by sentence. Lines said before are free.
const LIVE_CALLS: usize = 400;
/// Lines kept spoken, so a repeated one costs nothing.
const CACHE_LINES: usize = 256;
/// Most audio the cache holds in all.
const CACHE_BYTES: usize = 32 * 1024 * 1024;
const RATE_WINDOW: Duration = Duration::from_secs(10 * 60);
/// Longest audio a note may come back as, so the reply fits a frame.
const MAX_SPEECH: usize = 4 * 1024 * 1024;
/// Most of a notice the rewrite reads, in characters.
const MAX_INPUT_CHARS: usize = 4000;
/// Longest text spoken, in characters: about half a minute.
const MAX_SPOKEN_CHARS: usize = 500;

const REWRITE: &str = "You turn a message from Beep, a personal assistant, into a short voice note \
that Beep speaks to the user. Write one to three short sentences of plain spoken text: no markdown, \
lists, emoji, links or URLs. Say numbers, times, dates and amounts the way a person says them aloud. \
Keep the names and the one thing the user may want to act on, and drop details a listener cannot use. \
When the message asks the user something or offers to do something, end with that question. \
Write in the language of the message, and answer with the voice note only.";

/// The models voice notes use, from the server config's `[voice]` table.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct VoiceConfig {
    /// Speaks notes, such as `openrouter:x-ai/grok-voice-tts-1.0`.
    pub speech_model: Model,
    /// One of the speech model's voices.
    pub voice: String,
    /// Transcribes replies when the server has no `mistral_key`.
    pub transcription_model: Model,
    /// Mistral's model for replies, used instead with `mistral_key`.
    pub mistral_transcription_model: String,
    /// Mistral's realtime model for live words, used with `mistral_key`.
    pub realtime_model: String,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            speech_model: openrouter("x-ai/grok-voice-tts-1.0"),
            voice: "rex".to_owned(),
            transcription_model: openrouter("mistralai/voxtral-small-transcribe-3-0"),
            mistral_transcription_model: "voxtral-mini-transcribe-3-0".to_owned(),
            realtime_model: "voxtral-mini-transcribe-realtime-3-0".to_owned(),
        }
    }
}

fn openrouter(name: &str) -> Model {
    Model {
        provider: Provider::OpenRouter,
        model: AliasedModel {
            name: name.to_owned(),
            real_name: None,
        },
    }
}

/// Why a voice call failed. Only the kind reaches the client.
#[derive(Debug, Error)]
pub enum VoiceError {
    #[error("too many voice notes, try again in a few minutes")]
    RateLimited,
    #[error("voice notes are busy")]
    Busy,
    #[error("the recording is not audio Beep can read")]
    BadAudio,
    #[error("nothing was heard in the recording")]
    Silent,
    #[error("the voice provider failed")]
    Provider,
}

/// Speaks and transcribes through the provider, a bounded number of calls
/// at a time.
pub struct Voice {
    provider: Arc<dyn LlmProvider>,
    /// Mistral, which transcribes in place of `provider` when set.
    mistral: Option<Arc<dyn LlmProvider>>,
    config: VoiceConfig,
    permits: Semaphore,
    /// When each user's recent calls were made, oldest first.
    calls: Mutex<HashMap<String, VecDeque<Instant>>>,
    /// The same for live mode's lines.
    live_calls: Mutex<HashMap<String, VecDeque<Instant>>>,
    /// Live mode's lines, spoken before.
    said: Mutex<Said>,
}

/// Spoken lines by their text, oldest first out.
#[derive(Default)]
struct Said {
    audio: HashMap<String, Arc<Vec<u8>>>,
    order: VecDeque<String>,
    bytes: usize,
}

impl Said {
    fn get(&self, text: &str) -> Option<Arc<Vec<u8>>> {
        self.audio.get(text).cloned()
    }

    fn put(&mut self, text: &str, audio: Arc<Vec<u8>>) {
        if self.audio.contains_key(text) || audio.len() > CACHE_BYTES {
            return;
        }
        self.bytes += audio.len();
        self.audio.insert(text.to_owned(), audio);
        self.order.push_back(text.to_owned());
        while self.order.len() > CACHE_LINES || self.bytes > CACHE_BYTES {
            let Some(old) = self.order.pop_front() else {
                break;
            };
            if let Some(gone) = self.audio.remove(&old) {
                self.bytes -= gone.len();
            }
        }
    }
}

impl Voice {
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        mistral: Option<Arc<dyn LlmProvider>>,
        config: VoiceConfig,
    ) -> Self {
        Self {
            provider,
            mistral,
            config,
            permits: Semaphore::new(MAX_CALLS),
            calls: Mutex::default(),
            live_calls: Mutex::default(),
            said: Mutex::default(),
        }
    }

    /// `text` spoken, as MP3.
    pub async fn speak(&self, user: &str, text: &str) -> Result<Vec<u8>, VoiceError> {
        count(&self.calls, user, RATE_CALLS)?;
        self.synthesize(text).await
    }

    /// A line of live mode spoken, as MP3. Lines said before come from the
    /// cache and count against nothing.
    pub async fn say(&self, user: &str, text: &str) -> Result<Arc<Vec<u8>>, VoiceError> {
        let cached = self
            .said
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(text);
        if let Some(audio) = cached {
            return Ok(audio);
        }
        count(&self.live_calls, user, LIVE_CALLS)?;
        let audio = Arc::new(self.synthesize(text).await?);
        self.said
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .put(text, audio.clone());
        Ok(audio)
    }

    async fn synthesize(&self, text: &str) -> Result<Vec<u8>, VoiceError> {
        let request = TtsRequest {
            model: self.config.speech_model.capability_name().to_owned(),
            input: text.to_owned(),
            voice: self.config.voice.clone(),
            response_format: AudioFormat::Mp3,
        };
        let _permit = self.permit().await?;
        let mut stream = self.provider.text_to_speech(request);
        let mut audio = Vec::new();
        let read = async {
            while let Some(event) = stream.next().await {
                match event {
                    Ok(StreamEvent::AudioDelta { data, .. }) => {
                        audio.extend_from_slice(&data);
                        if audio.len() > MAX_SPEECH {
                            log::warn!("speech ran past {MAX_SPEECH} bytes");
                            return Err(VoiceError::Provider);
                        }
                    }
                    Ok(StreamEvent::Finished(_)) => break,
                    Ok(_) => {}
                    Err(error) => return Err(provider_failed("speech", &error)),
                }
            }
            Ok(())
        };
        match timeout(CALL_TIMEOUT, read).await {
            Ok(result) => result?,
            Err(_) => {
                log::warn!("speech timed out");
                return Err(VoiceError::Provider);
            }
        }
        if audio.is_empty() {
            log::warn!("speech came back empty");
            return Err(VoiceError::Provider);
        }
        Ok(audio)
    }

    /// What was said in `audio`.
    pub async fn transcribe(&self, user: &str, audio: &Upload) -> Result<String, VoiceError> {
        let media_type = voice_type(audio)?;
        count(&self.calls, user, RATE_CALLS)?;
        let (provider, model) = match &self.mistral {
            Some(mistral) => (mistral, self.config.mistral_transcription_model.as_str()),
            None => (
                &self.provider,
                self.config.transcription_model.capability_name(),
            ),
        };
        let request = SttRequest {
            model: model.to_owned(),
            audio: audio.data.clone(),
            media_type: media_type.to_owned(),
            language: None,
        };
        let _permit = self.permit().await?;
        let mut stream = provider.transcribe(request);
        let mut text = String::new();
        let read = async {
            while let Some(event) = stream.next().await {
                match event {
                    Ok(StreamEvent::ContentDelta(delta)) => text.push_str(&delta),
                    Ok(StreamEvent::Finished(_)) => break,
                    Ok(_) => {}
                    Err(error) => return Err(provider_failed("transcription", &error)),
                }
            }
            Ok(())
        };
        match timeout(CALL_TIMEOUT, read).await {
            Ok(result) => result?,
            Err(_) => {
                log::warn!("transcription timed out");
                return Err(VoiceError::Provider);
            }
        }
        let text = text.trim();
        if text.is_empty() {
            return Err(VoiceError::Silent);
        }
        Ok(text.to_owned())
    }

    async fn permit(&self) -> Result<tokio::sync::SemaphorePermit<'_>, VoiceError> {
        match timeout(QUEUE_WAIT, self.permits.acquire()).await {
            Ok(Ok(permit)) => Ok(permit),
            _ => Err(VoiceError::Busy),
        }
    }
}

/// Counts a call against `user` in `calls`, refusing one past `limit`
/// within `RATE_WINDOW`.
fn count(
    calls: &Mutex<HashMap<String, VecDeque<Instant>>>,
    user: &str,
    limit: usize,
) -> Result<(), VoiceError> {
    let mut calls = calls.lock().unwrap_or_else(PoisonError::into_inner);
    let now = Instant::now();
    // Users whose calls all aged out are dropped, so the map stays small.
    calls.retain(|_, times| {
        while times
            .front()
            .is_some_and(|at| now.duration_since(*at) >= RATE_WINDOW)
        {
            times.pop_front();
        }
        !times.is_empty()
    });
    let times = calls.entry(user.to_owned()).or_default();
    if times.len() >= limit {
        return Err(VoiceError::RateLimited);
    }
    times.push_back(now);
    Ok(())
}

/// Logs a provider failure without its body, which can echo the request.
fn provider_failed(what: &str, error: &anyhow::Error) -> VoiceError {
    let status = error
        .to_string()
        .split_once('(')
        .and_then(|(_, rest)| rest.split_once(')'))
        .map(|(status, _)| status.to_owned())
        .unwrap_or_default();
    log::warn!("{what} failed; status={status}");
    VoiceError::Provider
}

/// The type to send `audio` as, or `BadAudio` for an empty, oversized or
/// unknown recording. The provider client names the upload after the
/// subtype, so aliases become their usual name.
fn voice_type(audio: &Upload) -> Result<&'static str, VoiceError> {
    if audio.data.is_empty() || audio.data.len() > MAX_VOICE {
        return Err(VoiceError::BadAudio);
    }
    let mime = audio
        .mime
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let known = VOICE_TYPES
        .iter()
        .find(|known| **known == mime)
        .ok_or(VoiceError::BadAudio)?;
    Ok(match *known {
        "audio/x-wav" => "audio/wav",
        "audio/m4a" => "audio/mp4",
        other => other,
    })
}

/// The words a voice note says for a notice: the model's rewrite for
/// listening, or the notice made plain when the model fails.
pub async fn spoken_text(generator: &Generator<'_>, title: &str, body: &str) -> String {
    let message = cut(&plain(body), MAX_INPUT_CHARS);
    let prompt = if title.trim().is_empty() {
        format!("Message:\n{message}")
    } else {
        format!("Title: {}\nMessage:\n{message}", title.trim())
    };
    let rewrite = timeout(REWRITE_TIMEOUT, generator.text_up_to(REWRITE, prompt, 600)).await;
    let spoken = match rewrite {
        Ok(Ok(text)) => plain(&text),
        Ok(Err(error)) => {
            log::info!("rewriting a voice note failed: {error}");
            String::new()
        }
        Err(_) => {
            log::info!("rewriting a voice note timed out");
            String::new()
        }
    };
    let spoken = if spoken.is_empty() {
        fallback(title, body)
    } else {
        spoken
    };
    cut(&spoken, MAX_SPOKEN_CHARS)
}

/// A notice spoken as it is, made plain, for when the rewrite fails.
fn fallback(title: &str, body: &str) -> String {
    let body = plain(body);
    let title = plain(title);
    match (title.is_empty(), body.is_empty()) {
        (_, true) => title,
        (true, false) => body,
        (false, false) => {
            let stop = if title.ends_with(['.', '!', '?']) {
                ""
            } else {
                "."
            };
            format!("{title}{stop} {body}")
        }
    }
}

/// Text with markdown marks, links and line breaks taken out, which a
/// voice would read out literally.
fn plain(text: &str) -> String {
    let mut out = Vec::new();
    for word in text.split_whitespace() {
        // A markdown link keeps its words: [the invoice](https://...).
        let word = match word.find("](") {
            Some(at) => &word[..at],
            None => word,
        };
        let bare = word.trim_matches(|c: char| "([<\"'".contains(c));
        if bare.starts_with("http://") || bare.starts_with("https://") || bare.starts_with("www.") {
            continue;
        }
        let word: String = word
            .chars()
            .filter(|c| !matches!(c, '*' | '_' | '`' | '#' | '[' | ']' | '>' | '|' | '~'))
            .collect();
        if !word.is_empty() && word != "-" {
            out.push(word);
        }
    }
    out.join(" ")
}

/// `text` cut to at most `max` characters, at the end of a sentence when
/// one ends in the last third.
fn cut(text: &str, max: usize) -> String {
    let Some((end, _)) = text.char_indices().nth(max) else {
        return text.to_owned();
    };
    let head = &text[..end];
    match head.rfind(['.', '!', '?']) {
        Some(stop) if stop >= head.len() * 2 / 3 => head[..=stop].to_owned(),
        _ => format!("{}...", head.trim_end()),
    }
}

#[cfg(test)]
mod tests {
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

    fn wav(data: Vec<u8>) -> Upload {
        Upload {
            name: "reply.wav".into(),
            mime: "audio/x-wav".into(),
            data,
        }
    }

    fn llm() -> AgentLlmConfig {
        AgentLlmConfig::new("openrouter:test/model".parse().expect("a valid model"))
    }

    #[tokio::test]
    async fn refuses_bad_audio_before_calling() {
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
    async fn a_failed_or_silent_transcription_says_only_what_happened() {
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
    async fn a_user_is_limited_but_others_are_not() {
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
    async fn a_failed_rewrite_speaks_the_notice_made_plain() {
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
}
