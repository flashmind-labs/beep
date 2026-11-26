//! Desktop voice dictation: streams 16 kHz mono PCM from the composer to an
//! on-device `transcribe-cpp` model (`Parakeet Unified` or `Nemotron 3.5`) or a
//! cloud realtime stream (`ElevenLabs` or `Mistral`) with the API keys saved in
//! the mesh. Secrets never leave this process.

use crate::{
    chat::local_dictation::{
        AudioCommand, DictationEvent, LocalDictation, LocalModelId, ModelPaths,
        installed_model_path, next_audio, run_local,
    },
    error::{DesktopError, Result},
    fleet::projects::{FleetState, desk},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use beep_proto::ApiKey;
use bytes::Bytes;
use flashmind_llm::{ElevenLabsProvider, MistralProvider, elevenlabs, mistral};
use futures::StreamExt as _;
use secrecy::SecretString;
use std::{collections::HashMap, path::PathBuf, sync::Mutex, time::Duration};
use tauri::{Manager as _, State, Window, ipc::Channel};
use tokio::{sync::mpsc, time::timeout};
use url::Url;
use uuid::Uuid;

const AUDIO_QUEUE: usize = 128;
const MAX_CHUNK_BYTES: usize = 262_144;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);
const DRAIN_TIMEOUT: Duration = Duration::from_millis(2500);

const DEFAULT_ELEVENLABS_MODEL: &str = "scribe_v3_live";
const DEFAULT_MISTRAL_MODEL: &str = "voxtral-mini-transcribe-realtime-3-0";

const ELEVENLABS_BASE: &str = "https://api.elevenlabs.io/";
const MISTRAL_BASE: &str = "https://api.mistral.ai/";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Local { model: LocalModelId },
    ElevenLabs { model: String },
    Mistral { model: String },
}

pub(crate) enum Credential {
    Local(PathBuf),
    ApiKey(SecretString),
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
    local: State<'_, LocalDictation>,
    model: Option<String>,
    on_event: Channel<DictationEvent>,
) -> Result<Uuid> {
    let desk = desk(&fleet).await?;
    let chosen = model
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .or_else(|| desk.dictation().model);
    let paths = ModelPaths::from_app(window.app_handle())?;
    let installed_paths: HashMap<LocalModelId, PathBuf> = tokio::task::spawn_blocking(move || {
        LocalModelId::ALL
            .into_iter()
            .filter_map(|id| installed_model_path(&paths, id).map(|path| (id, path)))
            .collect()
    })
    .await
    .map_err(|e| DesktopError::Other(e.into()))?;

    let (target, credential) = resolve_model(
        chosen.as_deref(),
        |id| installed_paths.get(&id).cloned(),
        |provider| desk.api_key_secret(provider),
    )?;

    let id = Uuid::new_v4();
    let (audio_tx, audio_rx) = mpsc::channel(AUDIO_QUEUE);
    let label = window.label().to_owned();
    let app = window.app_handle().clone();
    let local_state = local.inner().clone();

    let task = tauri::async_runtime::spawn(async move {
        run_dictation(
            target,
            credential,
            local_state,
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
    mut local_path: impl FnMut(LocalModelId) -> Option<PathBuf>,
    mut key_for: impl FnMut(ApiKey) -> Option<SecretString>,
) -> Result<(Target, Credential)> {
    let explicit = model
        .map(str::trim)
        .filter(|raw| !raw.is_empty() && !raw.starts_with("openrouter:"));

    if let Some(raw) = explicit {
        let target = parse_target(raw)?;
        return match target {
            Target::Local { model } => {
                let path = local_path(model).ok_or_else(|| {
                    DesktopError::BadRequest(format!(
                        "Download {} in Settings > General first.",
                        model.display_name()
                    ))
                })?;
                Ok((target, Credential::Local(path)))
            }
            Target::ElevenLabs { .. } => {
                let secret = key_for(ApiKey::ElevenLabs).ok_or_else(|| {
                    DesktopError::BadRequest(
                        "Add your ElevenLabs API key in Settings first.".into(),
                    )
                })?;
                Ok((target, Credential::ApiKey(secret)))
            }
            Target::Mistral { .. } => {
                let secret = key_for(ApiKey::Mistral).ok_or_else(|| {
                    DesktopError::BadRequest("Add your Mistral API key in Settings first.".into())
                })?;
                Ok((target, Credential::ApiKey(secret)))
            }
        };
    }

    for id in LocalModelId::ALL {
        if let Some(path) = local_path(id) {
            return Ok((Target::Local { model: id }, Credential::Local(path)));
        }
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
    ] {
        if let Some(secret) = key_for(provider) {
            return Ok((default, Credential::ApiKey(secret)));
        }
    }

    Err(DesktopError::BadRequest(
        "Download a local model or add an ElevenLabs or Mistral API key in Settings first.".into(),
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
        "local" => LocalModelId::from_slug(slug)
            .map(|model| Target::Local { model })
            .ok_or_else(|| DesktopError::BadRequest("Unknown local dictation model.".into())),
        "elevenlabs" => Ok(Target::ElevenLabs {
            model: slug.to_owned(),
        }),
        "mistral" => Ok(Target::Mistral {
            model: slug.to_owned(),
        }),
        _ => Err(DesktopError::BadRequest(
            "Unsupported dictation provider.".into(),
        )),
    }
}

pub(crate) async fn run_dictation(
    target: Target,
    credential: Credential,
    local: LocalDictation,
    audio: mpsc::Receiver<AudioCommand>,
    mut emit: impl FnMut(DictationEvent) + Send,
    urls: Urls,
) {
    let outcome = match (target, credential) {
        (Target::Local { model }, Credential::Local(path)) => {
            run_local(local, model, path, audio, &mut emit).await
        }
        (Target::ElevenLabs { model }, Credential::ApiKey(secret)) => {
            run_elevenlabs(&urls.elevenlabs, model, secret, audio, &mut emit).await
        }
        (Target::Mistral { model }, Credential::ApiKey(secret)) => {
            run_mistral(&urls.mistral, model, secret, audio, &mut emit).await
        }
        _ => Err("Mismatched dictation credential.".into()),
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
