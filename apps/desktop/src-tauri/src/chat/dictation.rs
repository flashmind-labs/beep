//! Desktop voice dictation: streams 16 kHz mono PCM from the composer to
//! ElevenLabs or Mistral realtime transcription, or phrase by phrase to
//! OpenRouter, through `flashmind-llm` with the API keys saved in the mesh.
//! Secrets never leave this process.

use crate::{
    error::{DesktopError, Result},
    fleet::projects::{FleetState, desk},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use beep_proto::ApiKey;
use bytes::Bytes;
use flashmind_llm::{
    ElevenLabsProvider, MistralProvider, OpenRouterProvider, elevenlabs, mistral,
    openrouter::TranscriptionOptions,
};
use flashmind_types::SttRequest;
use futures::StreamExt as _;
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use std::{collections::HashMap, sync::Mutex, time::Duration};
use tauri::{Manager as _, State, Window, ipc::Channel};
use tokio::{sync::mpsc, time::timeout};
use url::Url;
use uuid::Uuid;

const SAMPLE_RATE: u32 = 16_000;
const AUDIO_QUEUE: usize = 128;
const MAX_CHUNK_BYTES: usize = 262_144;
/// Fifteen minutes of 16 kHz 16-bit mono audio.
const MAX_SESSION_PCM_BYTES: usize = 32_000 * 60 * 15;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);
const DRAIN_TIMEOUT: Duration = Duration::from_millis(2500);
const OPENROUTER_TIMEOUT: Duration = Duration::from_secs(45);

/// RMS threshold squared on signed 16-bit PCM (`280^2`) separating voice from room floor.
const SPEECH_MEAN_SQUARE: u64 = 78_400;
/// At least 400 ms of speech before silence ends a phrase segment on OpenRouter.
const MIN_SPEECH_BYTES: usize = 12_800;
/// 200 ms of audio kept ahead of the first voiced chunk.
const PRE_ROLL_BYTES: usize = 6_400;
/// 850 ms of trailing silence triggers an interim phrase upload on OpenRouter.
const SILENCE_FLUSH_BYTES: usize = 27_200;
/// Cap one OpenRouter interim segment at 12 seconds so long sentences still appear live.
const MAX_SEGMENT_BYTES: usize = 384_000;

const DEFAULT_ELEVENLABS_MODEL: &str = "scribe_v3_live";
const DEFAULT_MISTRAL_MODEL: &str = "voxtral-mini-transcribe-realtime-3-0";
const DEFAULT_OPENROUTER_MODEL: &str = "openai/whisper-large-v3-turbo";

const ELEVENLABS_BASE: &str = "https://api.elevenlabs.io/";
const MISTRAL_BASE: &str = "https://api.mistral.ai/";

/// Events streamed back to the composer over a Tauri channel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DictationEvent {
    /// Connected and ready for microphone PCM.
    Ready,
    /// Updated transcript for this dictation turn: finalized words plus current interim segment.
    Transcript { committed: String, partial: String },
    /// Finishing transcription after a pause or stop.
    Thinking,
    /// Session ended cleanly.
    Done { nothing_heard: bool },
    /// Session failed with a user-facing message.
    Error { message: String },
}

pub(crate) enum AudioCommand {
    Append(Bytes),
    Stop,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    ElevenLabs { model: String },
    Mistral { model: String },
    OpenRouter { model: String },
}

impl Target {
    fn provider(&self) -> ApiKey {
        match self {
            Self::ElevenLabs { .. } => ApiKey::ElevenLabs,
            Self::Mistral { .. } => ApiKey::Mistral,
            Self::OpenRouter { .. } => ApiKey::OpenRouter,
        }
    }
}

struct Running {
    window: String,
    audio: mpsc::Sender<AudioCommand>,
    task: tauri::async_runtime::JoinHandle<()>,
}

/// Active dictation sessions across open windows, at most one per window.
#[derive(Default)]
pub struct Dictations {
    sessions: Mutex<HashMap<Uuid, Running>>,
}

impl Dictations {
    /// Stops every session owned by `window`.
    pub fn clear(&self, window: &str) {
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let ids: Vec<Uuid> = sessions
            .iter()
            .filter(|(_, running)| running.window == window)
            .map(|(&id, _)| id)
            .collect();

        for id in ids {
            if let Some(running) = sessions.remove(&id) {
                running.task.abort();
            }
        }
    }

    fn insert(&self, id: Uuid, running: Running) {
        self.clear(&running.window);
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        sessions.insert(id, running);
    }

    fn sender(&self, id: Uuid) -> Option<mpsc::Sender<AudioCommand>> {
        let sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        sessions.get(&id).map(|running| running.audio.clone())
    }

    fn take_sender(&self, id: Uuid) -> Option<mpsc::Sender<AudioCommand>> {
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        sessions.remove(&id).map(|running| running.audio)
    }

    fn abort(&self, id: Uuid) {
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(running) = sessions.remove(&id) {
            running.task.abort();
        }
    }

    fn finish(&self, id: Uuid) {
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        sessions.remove(&id);
    }
}

#[tauri::command]
pub async fn dictation_start(
    window: Window,
    fleet: FleetState<'_>,
    dictations: State<'_, Dictations>,
    model: Option<String>,
    on_event: Channel<DictationEvent>,
) -> Result<Uuid> {
    let desk = desk(&fleet).await?;
    let chosen = model
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .or_else(|| desk.dictation().model);
    let (target, secret) =
        resolve_model(chosen.as_deref(), |provider| desk.api_key_secret(provider))?;

    let id = Uuid::new_v4();
    let (audio_tx, audio_rx) = mpsc::channel(AUDIO_QUEUE);
    let label = window.label().to_owned();
    let app = window.app_handle().clone();

    let task = tauri::async_runtime::spawn(async move {
        run_dictation(
            target,
            secret,
            audio_rx,
            move |event| {
                let _ = on_event.send(event);
            },
            Urls::production(),
        )
        .await;
        app.state::<Dictations>().finish(id);
    });

    dictations.insert(
        id,
        Running {
            window: label,
            audio: audio_tx,
            task,
        },
    );
    Ok(id)
}

#[tauri::command]
pub async fn dictation_audio(
    dictations: State<'_, Dictations>,
    id: Uuid,
    pcm_base64: String,
) -> Result<()> {
    let bytes = STANDARD
        .decode(pcm_base64.trim())
        .map_err(|_| DesktopError::BadRequest("Invalid audio chunk.".into()))?;

    if bytes.is_empty() {
        return Ok(());
    }

    if bytes.len() > MAX_CHUNK_BYTES || bytes.len() % 2 != 0 {
        return Err(DesktopError::BadRequest(
            "Audio chunk must be 16-bit PCM within 256 KB.".into(),
        ));
    }

    let Some(sender) = dictations.sender(id) else {
        return Ok(());
    };
    let _ = sender.send(AudioCommand::Append(Bytes::from(bytes))).await;
    Ok(())
}

#[tauri::command]
pub async fn dictation_stop(dictations: State<'_, Dictations>, id: Uuid) -> Result<()> {
    if let Some(sender) = dictations.take_sender(id) {
        let _ = sender.send(AudioCommand::Stop).await;
    }

    Ok(())
}

#[tauri::command]
pub fn dictation_cancel(dictations: State<'_, Dictations>, id: Uuid) {
    dictations.abort(id);
}

#[derive(Clone)]
pub(crate) struct Urls {
    pub elevenlabs: Url,
    pub mistral: Url,
}

impl Urls {
    fn production() -> Self {
        Self {
            elevenlabs: Url::parse(ELEVENLABS_BASE).expect("static ElevenLabs URL is valid"),
            mistral: Url::parse(MISTRAL_BASE).expect("static Mistral URL is valid"),
        }
    }
}

pub(crate) fn resolve_model(
    model: Option<&str>,
    mut key_for: impl FnMut(ApiKey) -> Option<SecretString>,
) -> Result<(Target, SecretString)> {
    if let Some(raw) = model.map(str::trim).filter(|s| !s.is_empty()) {
        let target = parse_target(raw)?;
        let provider = target.provider();
        let secret = key_for(provider).ok_or_else(|| {
            DesktopError::BadRequest(format!(
                "Add your {} API key in Settings first.",
                provider.human_readable_name()
            ))
        })?;
        return Ok((target, secret));
    }

    for (provider, default) in [
        (
            ApiKey::ElevenLabs,
            Target::ElevenLabs {
                model: DEFAULT_ELEVENLABS_MODEL.to_owned(),
            },
        ),
        (
            ApiKey::Mistral,
            Target::Mistral {
                model: DEFAULT_MISTRAL_MODEL.to_owned(),
            },
        ),
        (
            ApiKey::OpenRouter,
            Target::OpenRouter {
                model: DEFAULT_OPENROUTER_MODEL.to_owned(),
            },
        ),
    ] {
        if let Some(secret) = key_for(provider) {
            return Ok((default, secret));
        }
    }

    Err(DesktopError::BadRequest(
        "Add an ElevenLabs, Mistral or OpenRouter API key in Settings first.".into(),
    ))
}

pub(crate) fn parse_target(model: &str) -> Result<Target> {
    let (prefix, slug) = model.split_once(':').unwrap_or(("elevenlabs", model));
    let slug = slug.trim();

    if slug.is_empty() || slug.len() > 128 || slug.chars().any(char::is_whitespace) {
        return Err(DesktopError::BadRequest(
            "Pick a valid dictation model.".into(),
        ));
    }

    match prefix {
        "elevenlabs" => Ok(Target::ElevenLabs {
            model: slug.to_owned(),
        }),
        "mistral" => Ok(Target::Mistral {
            model: slug.to_owned(),
        }),
        "openrouter" => Ok(Target::OpenRouter {
            model: slug.to_owned(),
        }),
        _ => Err(DesktopError::BadRequest(
            "Unsupported dictation provider.".into(),
        )),
    }
}

pub(crate) async fn run_dictation(
    target: Target,
    secret: SecretString,
    audio: mpsc::Receiver<AudioCommand>,
    mut emit: impl FnMut(DictationEvent) + Send,
    urls: Urls,
) {
    let outcome = match target {
        Target::ElevenLabs { model } => {
            run_elevenlabs(&urls.elevenlabs, model, secret, audio, &mut emit).await
        }
        Target::Mistral { model } => {
            run_mistral(&urls.mistral, model, secret, audio, &mut emit).await
        }
        Target::OpenRouter { model } => run_openrouter(model, secret, audio, &mut emit).await,
    };

    match outcome {
        Ok(nothing_heard) => emit(DictationEvent::Done { nothing_heard }),
        Err(message) => emit(DictationEvent::Error { message }),
    }
}

/// Committed words plus the current interim segment of one dictation turn.
#[derive(Default)]
struct Transcript {
    committed: String,
    partial: String,
}

impl Transcript {
    fn event(&self) -> DictationEvent {
        DictationEvent::Transcript {
            committed: self.committed.clone(),
            partial: self.partial.clone(),
        }
    }

    fn commit(&mut self, segment: &str) {
        append_segment(&mut self.committed, segment);
        self.partial.clear();
    }

    /// Keeps an unfinished interim segment once the session ends, and reports whether nothing
    /// was heard.
    fn settle(mut self, emit: &mut impl FnMut(DictationEvent)) -> bool {
        if !self.partial.is_empty() {
            let partial = std::mem::take(&mut self.partial);
            self.commit(&partial);
            emit(self.event());
        }

        self.committed.trim().is_empty()
    }
}

/// The next microphone chunk, or `None` once the user stops or the session limit is reached.
fn next_audio(command: Option<AudioCommand>, sent: &mut usize) -> Option<Bytes> {
    let Some(AudioCommand::Append(pcm)) = command else {
        return None;
    };

    *sent = sent.saturating_add(pcm.len());
    (*sent <= MAX_SESSION_PCM_BYTES).then_some(pcm)
}

async fn run_elevenlabs(
    base: &Url,
    model: String,
    secret: SecretString,
    mut audio: mpsc::Receiver<AudioCommand>,
    emit: &mut (impl FnMut(DictationEvent) + Send),
) -> std::result::Result<bool, String> {
    let provider = ElevenLabsProvider::new(secret)
        .map_err(|e| format!("{e:#}"))?
        .with_base_url(base.clone());
    let request = elevenlabs::RealtimeTranscriptionRequest::new(model)
        .with_commit_strategy(elevenlabs::CommitStrategy::Vad);
    let session = timeout(HANDSHAKE_TIMEOUT, provider.realtime_transcription(request))
        .await
        .map_err(|_| "Timed out waiting for ElevenLabs.".to_owned())?
        .map_err(|e| format!("Could not start ElevenLabs dictation: {e:#}"))?;
    let (sink, mut events) = session.split();
    emit(DictationEvent::Ready);

    let mut transcript = Transcript::default();
    let mut sent = 0;
    loop {
        tokio::select! {
            command = audio.recv() => match next_audio(command, &mut sent) {
                Some(pcm) => sink
                    .send(pcm)
                    .await
                    .map_err(|e| format!("Lost connection to ElevenLabs: {e:#}"))?,
                None => break,
            },
            event = events.next() => match event {
                Some(Ok(event)) => apply_elevenlabs(event, &mut transcript, emit),
                Some(Err(e)) => return Err(format!("{e:#}")),
                None => return Err("ElevenLabs closed the transcription stream.".into()),
            },
        }
    }

    emit(DictationEvent::Thinking);
    let _ = sink.finish().await;
    let _ = timeout(DRAIN_TIMEOUT, async {
        while let Some(Ok(event)) = events.next().await {
            apply_elevenlabs(event, &mut transcript, emit);
        }
    })
    .await;

    Ok(transcript.settle(emit))
}

fn apply_elevenlabs(
    event: elevenlabs::TranscriptionEvent,
    transcript: &mut Transcript,
    emit: &mut impl FnMut(DictationEvent),
) {
    match event {
        elevenlabs::TranscriptionEvent::Partial(text) => {
            transcript.partial = text.trim().to_owned();
            emit(transcript.event());
        }
        elevenlabs::TranscriptionEvent::Committed(text) => {
            transcript.commit(&text);
            emit(transcript.event());
        }
        _ => {}
    }
}

async fn run_mistral(
    base: &Url,
    model: String,
    secret: SecretString,
    mut audio: mpsc::Receiver<AudioCommand>,
    emit: &mut (impl FnMut(DictationEvent) + Send),
) -> std::result::Result<bool, String> {
    let provider = MistralProvider::new(secret)
        .with_base_url(base.as_str())
        .map_err(|e| format!("{e:#}"))?;
    let request = mistral::RealtimeTranscriptionRequest::new(model);
    let session = timeout(HANDSHAKE_TIMEOUT, provider.realtime_transcription(request))
        .await
        .map_err(|_| "Timed out waiting for Mistral.".to_owned())?
        .map_err(|e| format!("Could not start Mistral dictation: {e:#}"))?;
    let (sink, mut events) = session.split();
    emit(DictationEvent::Ready);

    let mut transcript = Transcript::default();
    let mut sent = 0;
    loop {
        tokio::select! {
            command = audio.recv() => match next_audio(command, &mut sent) {
                Some(pcm) => sink
                    .send(pcm)
                    .await
                    .map_err(|e| format!("Lost connection to Mistral: {e:#}"))?,
                None => break,
            },
            event = events.next() => match event {
                Some(Ok(event)) => apply_mistral(event, &mut transcript, emit),
                Some(Err(e)) => return Err(format!("{e:#}")),
                None => return Err("Mistral closed the transcription stream.".into()),
            },
        }
    }

    emit(DictationEvent::Thinking);
    let _ = sink.finish().await;
    let _ = timeout(DRAIN_TIMEOUT, async {
        while let Some(Ok(event)) = events.next().await {
            apply_mistral(event, &mut transcript, emit);
        }
    })
    .await;

    Ok(transcript.settle(emit))
}

fn apply_mistral(
    event: mistral::TranscriptionEvent,
    transcript: &mut Transcript,
    emit: &mut impl FnMut(DictationEvent),
) {
    match event {
        mistral::TranscriptionEvent::TextDelta(text) => {
            append_delta(&mut transcript.committed, &text);
            emit(transcript.event());
        }
        mistral::TranscriptionEvent::Done(done)
            if transcript.committed.trim().is_empty() && !done.text.trim().is_empty() =>
        {
            transcript.committed = done.text.trim().to_owned();
            emit(transcript.event());
        }
        _ => {}
    }
}

/// OpenRouter only transcribes whole files, so each pause after speech uploads one phrase.
async fn run_openrouter(
    model: String,
    secret: SecretString,
    mut audio: mpsc::Receiver<AudioCommand>,
    emit: &mut (impl FnMut(DictationEvent) + Send),
) -> std::result::Result<bool, String> {
    let provider = OpenRouterProvider::new(secret.expose_secret().to_owned());
    emit(DictationEvent::Ready);

    let mut transcript = Transcript::default();
    let mut segment: Vec<u8> = Vec::new();
    let mut speech_bytes: usize = 0;
    let mut silence_bytes: usize = 0;
    let mut sent = 0;

    while let Some(pcm) = next_audio(audio.recv().await, &mut sent) {
        let voiced = pcm_is_voiced(&pcm);
        segment.extend_from_slice(&pcm);
        if voiced {
            speech_bytes = speech_bytes.saturating_add(pcm.len());
            silence_bytes = 0;
        } else if speech_bytes > 0 {
            silence_bytes = silence_bytes.saturating_add(pcm.len());
        } else if segment.len() > PRE_ROLL_BYTES {
            segment.drain(..segment.len() - PRE_ROLL_BYTES);
        }

        let phrase_ended =
            silence_bytes >= SILENCE_FLUSH_BYTES || segment.len() >= MAX_SEGMENT_BYTES;
        if speech_bytes >= MIN_SPEECH_BYTES && phrase_ended {
            emit(DictationEvent::Thinking);
            let text =
                transcribe_openrouter(&provider, &model, std::mem::take(&mut segment)).await?;
            speech_bytes = 0;
            silence_bytes = 0;
            transcript.commit(&text);
            emit(transcript.event());
        }
    }

    if speech_bytes > 0 && !segment.is_empty() {
        emit(DictationEvent::Thinking);
        let text = transcribe_openrouter(&provider, &model, segment).await?;
        transcript.commit(&text);
        emit(transcript.event());
    }

    Ok(transcript.settle(emit))
}

async fn transcribe_openrouter(
    provider: &OpenRouterProvider,
    model: &str,
    pcm: Vec<u8>,
) -> std::result::Result<String, String> {
    let request = SttRequest {
        model: model.to_owned(),
        audio: encode_wav_pcm16_mono(&pcm, SAMPLE_RATE),
        media_type: "audio/wav".to_owned(),
        language: None,
    };
    let response = timeout(
        OPENROUTER_TIMEOUT,
        provider.transcribe_detailed(request, TranscriptionOptions::default()),
    )
    .await
    .map_err(|_| "OpenRouter transcription timed out.".to_owned())?
    .map_err(|e| format!("OpenRouter transcription failed: {e:#}"))?;

    Ok(response.text)
}

pub(crate) fn pcm_is_voiced(pcm: &[u8]) -> bool {
    let samples = pcm.len() / 2;
    if samples == 0 {
        return false;
    }

    let mut sum_sq: u64 = 0;
    for pair in pcm.as_chunks::<2>().0 {
        let sample = i64::from(i16::from_le_bytes(*pair));
        sum_sq = sum_sq.saturating_add(sample.unsigned_abs().saturating_mul(sample.unsigned_abs()));
    }

    sum_sq / (samples as u64) >= SPEECH_MEAN_SQUARE
}

pub(crate) fn encode_wav_pcm16_mono(pcm: &[u8], sample_rate: u32) -> Vec<u8> {
    let data_len = u32::try_from(pcm.len()).unwrap_or(u32::MAX);
    let riff_len = data_len.saturating_add(36);
    let byte_rate = sample_rate.saturating_mul(2);
    let mut out = Vec::with_capacity(pcm.len().saturating_add(44));
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_len.to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(pcm);
    out
}

fn append_segment(committed: &mut String, segment: &str) {
    let trimmed = segment.trim();
    if trimmed.is_empty() {
        return;
    }

    if !committed.is_empty() && !committed.ends_with(char::is_whitespace) {
        committed.push(' ');
    }
    committed.push_str(trimmed);
}

fn append_delta(committed: &mut String, delta: &str) {
    if delta.is_empty() {
        return;
    }

    if committed.is_empty() {
        committed.push_str(delta.trim_start());
    } else {
        committed.push_str(delta);
    }
}

#[cfg(test)]
#[path = "../tests/chat_dictation.rs"]
mod tests;
