//! On-device real-time speech-to-text models (`Parakeet Unified` and
//! `Nemotron 3.5`), downloaded on demand as quantized GGUF files and streamed
//! through `transcribe-cpp`.

use crate::store::lock;
use crate::{
    beep,
    error::{DesktopError, Result},
};
use bytes::Bytes;
use serde::Serialize;
use sha2::{Digest as _, Sha256};
use std::{
    collections::HashMap,
    fmt, fs,
    io::Read as _,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{
        Arc, Mutex, Once,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager as _, State, ipc::Channel};
use tokio::{io::AsyncWriteExt as _, sync::mpsc};

/// Fifteen minutes of 16 kHz 16-bit mono audio.
const MAX_SESSION_PCM_BYTES: usize = 32_000 * 60 * 15;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const READ_TIMEOUT: Duration = Duration::from_secs(60);
const PROGRESS_STEP_BYTES: u64 = 512 * 1024;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(80);
/// Coalesce up to one second of waiting 16 kHz mono 16-bit PCM (`32_000` bytes)
/// per `feed` call if CPU compute briefly lags behind microphone capture.
const MAX_COALESCED_PCM_BYTES: usize = 32_000;

static INIT_LOGGING: Once = Once::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LocalModelId {
    ParakeetUnified,
    Nemotron35Asr,
}

impl LocalModelId {
    pub const ALL: [Self; 2] = [Self::ParakeetUnified, Self::Nemotron35Asr];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ParakeetUnified => "local:parakeet-unified",
            Self::Nemotron35Asr => "local:nemotron-3.5-asr",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        match slug {
            "parakeet-unified" => Some(Self::ParakeetUnified),
            "nemotron-3.5-asr" => Some(Self::Nemotron35Asr),
            _ => None,
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::ParakeetUnified => "Parakeet Unified",
            Self::Nemotron35Asr => "Nemotron 3.5",
        }
    }

    pub const fn filename(self) -> &'static str {
        match self {
            Self::ParakeetUnified => "parakeet-unified-en-0.6b-Q8_0.gguf",
            Self::Nemotron35Asr => "nemotron-3.5-asr-streaming-0.6b-Q8_0.gguf",
        }
    }

    pub const fn url(self) -> &'static str {
        match self {
            Self::ParakeetUnified => {
                "https://huggingface.co/handy-computer/parakeet-unified-en-0.6b-gguf/resolve/7e948f21b7bdbac698d3318db9d350f1096f3b6c/parakeet-unified-en-0.6b-Q8_0.gguf"
            }
            Self::Nemotron35Asr => {
                "https://huggingface.co/handy-computer/nemotron-3.5-asr-streaming-0.6b-gguf/resolve/53ce288f69326b415aeef030cfcb02a08a8096dd/nemotron-3.5-asr-streaming-0.6b-Q8_0.gguf"
            }
        }
    }

    pub const fn bytes(self) -> u64 {
        match self {
            Self::ParakeetUnified => 731_357_568,
            Self::Nemotron35Asr => 751_094_240,
        }
    }

    pub const fn sha256(self) -> &'static str {
        match self {
            Self::ParakeetUnified => {
                "4b50b6dd862bf6e346929aaf4f5eaacec003bfa3f56462d6c874b41ef2f38795"
            }
            Self::Nemotron35Asr => {
                "b94545b313b3223fda7b2857a52681da813935c2127643d1e9ff0c23d988089c"
            }
        }
    }

    pub const fn language(self) -> Option<&'static str> {
        match self {
            Self::ParakeetUnified => Some("en"),
            Self::Nemotron35Asr => None,
        }
    }
}

impl fmt::Display for LocalModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for LocalModelId {
    type Err = DesktopError;

    fn from_str(raw: &str) -> Result<Self> {
        let slug = raw
            .trim()
            .strip_prefix("local:")
            .unwrap_or_else(|| raw.trim());
        Self::from_slug(slug)
            .ok_or_else(|| DesktopError::BadRequest("Unknown local dictation model.".into()))
    }
}

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

/// The next microphone chunk, or `None` once the user stops or the session limit is reached.
pub(crate) fn next_audio(command: Option<AudioCommand>, sent: &mut usize) -> Option<Bytes> {
    let Some(AudioCommand::Append(pcm)) = command else {
        return None;
    };

    *sent = sent.saturating_add(pcm.len());
    (*sent <= MAX_SESSION_PCM_BYTES).then_some(pcm)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LocalModelStatus {
    pub id: String,
    pub bytes: u64,
    pub installed: bool,
    pub downloading: bool,
    pub downloaded_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LocalDownloadEvent {
    Progress {
        downloaded_bytes: u64,
        total_bytes: u64,
    },
    Done,
    Cancelled,
    Error {
        message: String,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct ModelPaths {
    pub own_dir: PathBuf,
    pub hex_dir: Option<PathBuf>,
}

impl ModelPaths {
    pub fn from_app(app: &AppHandle) -> Result<Self> {
        let own_dir = beep::identity::data_dir(app)?.join("dictation-models");
        let hex_dir = app.path().data_dir().ok().map(|dir| dir.join("hex/models"));
        Ok(Self { own_dir, hex_dir })
    }
}

struct ActiveDownload {
    downloaded: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
    subscribers: Arc<Mutex<Vec<Channel<LocalDownloadEvent>>>>,
}

struct LoadedModel {
    id: LocalModelId,
    path: PathBuf,
    model: transcribe_cpp::Model,
}

struct Inner {
    downloads: Mutex<HashMap<LocalModelId, ActiveDownload>>,
    loaded: Mutex<Option<LoadedModel>>,
}

/// Shared state for local dictation downloads and cached model weights.
#[derive(Clone)]
pub struct LocalDictation {
    inner: Arc<Inner>,
}

impl Default for LocalDictation {
    fn default() -> Self {
        Self {
            inner: Arc::new(Inner {
                downloads: Mutex::new(HashMap::new()),
                loaded: Mutex::new(None),
            }),
        }
    }
}

impl LocalDictation {
    pub(crate) fn statuses(&self, paths: &ModelPaths) -> Vec<LocalModelStatus> {
        LocalModelId::ALL
            .into_iter()
            .map(|model| {
                let installed = installed_model_path(paths, model).is_some();
                let active = self.active_progress(model);
                LocalModelStatus {
                    id: model.as_str().to_owned(),
                    bytes: model.bytes(),
                    installed,
                    downloading: active.is_some(),
                    downloaded_bytes: active.unwrap_or(0),
                }
            })
            .collect()
    }

    fn active_progress(&self, model: LocalModelId) -> Option<u64> {
        let downloads = lock(&self.inner.downloads);
        downloads
            .get(&model)
            .map(|active| active.downloaded.load(Ordering::Relaxed))
    }

    fn cancel(&self, model: LocalModelId) {
        let removed = {
            let mut downloads = lock(&self.inner.downloads);
            downloads.remove(&model)
        };

        if let Some(active) = removed {
            active.cancel.store(true, Ordering::Relaxed);
            broadcast(&active.subscribers, LocalDownloadEvent::Cancelled);
        }
    }

    fn load_or_cached(
        &self,
        id: LocalModelId,
        path: &Path,
    ) -> std::result::Result<transcribe_cpp::Model, String> {
        INIT_LOGGING.call_once(transcribe_cpp::init_logging);

        {
            let loaded = lock(&self.inner.loaded);
            if let Some(entry) = loaded.as_ref()
                && entry.id == id
                && entry.path == path
            {
                return Ok(entry.model.clone());
            }
        }

        let model = transcribe_cpp::Model::load(path)
            .map_err(|e| format!("Could not load {}: {e}", id.display_name()))?;
        let mut loaded = lock(&self.inner.loaded);
        *loaded = Some(LoadedModel {
            id,
            path: path.to_path_buf(),
            model: model.clone(),
        });
        Ok(model)
    }
}

#[tauri::command]
pub async fn dictation_local_models(
    app: AppHandle,
    local: State<'_, LocalDictation>,
) -> Result<Vec<LocalModelStatus>> {
    let paths = ModelPaths::from_app(&app)?;
    let state = local.inner().clone();
    tokio::task::spawn_blocking(move || state.statuses(&paths))
        .await
        .map_err(|e| DesktopError::Other(e.into()))
}

#[tauri::command]
pub async fn dictation_model_download(
    app: AppHandle,
    local: State<'_, LocalDictation>,
    model: String,
    on_event: Channel<LocalDownloadEvent>,
) -> Result<()> {
    let id = LocalModelId::from_str(&model)?;
    let paths = ModelPaths::from_app(&app)?;

    let check_paths = paths.clone();
    let already_installed =
        tokio::task::spawn_blocking(move || installed_model_path(&check_paths, id).is_some())
            .await
            .map_err(|e| DesktopError::Other(e.into()))?;
    if already_installed {
        let _ = on_event.send(LocalDownloadEvent::Done);
        return Ok(());
    }

    let (downloaded, cancel, subscribers) = {
        let mut downloads = lock(&local.inner.downloads);
        if let Some(existing) = downloads.get(&id) {
            let so_far = existing.downloaded.load(Ordering::Relaxed);
            let _ = on_event.send(LocalDownloadEvent::Progress {
                downloaded_bytes: so_far,
                total_bytes: id.bytes(),
            });
            lock(&existing.subscribers).push(on_event);
            return Ok(());
        }

        let downloaded = Arc::new(AtomicU64::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let subscribers = Arc::new(Mutex::new(vec![on_event]));
        downloads.insert(
            id,
            ActiveDownload {
                downloaded: Arc::clone(&downloaded),
                cancel: Arc::clone(&cancel),
                subscribers: Arc::clone(&subscribers),
            },
        );
        (downloaded, cancel, subscribers)
    };

    let client = http_client()?;
    let state = local.inner().clone();
    tauri::async_runtime::spawn(async move {
        let progress_subscribers = Arc::clone(&subscribers);
        let progress_downloaded = Arc::clone(&downloaded);
        let outcome = download_verified(
            &client,
            VerifiedDownload {
                url: id.url(),
                expected_bytes: id.bytes(),
                expected_sha256: id.sha256(),
                dest_dir: &paths.own_dir,
                filename: id.filename(),
            },
            &cancel,
            move |done, total| {
                progress_downloaded.store(done, Ordering::Relaxed);
                broadcast(
                    &progress_subscribers,
                    LocalDownloadEvent::Progress {
                        downloaded_bytes: done,
                        total_bytes: total,
                    },
                );
            },
        )
        .await;

        let was_active = {
            let mut downloads = lock(&state.inner.downloads);
            downloads
                .get(&id)
                .is_some_and(|active| Arc::ptr_eq(&active.cancel, &cancel))
                .then(|| downloads.remove(&id))
                .flatten()
                .is_some()
        };

        if !was_active {
            return;
        }

        match outcome {
            Ok(()) => broadcast(&subscribers, LocalDownloadEvent::Done),
            Err(DownloadFailure::Cancelled) => {
                broadcast(&subscribers, LocalDownloadEvent::Cancelled);
            }
            Err(DownloadFailure::Failed(message)) => {
                broadcast(&subscribers, LocalDownloadEvent::Error { message });
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub fn dictation_model_cancel_download(
    local: State<'_, LocalDictation>,
    model: String,
) -> Result<()> {
    let id = LocalModelId::from_str(&model)?;
    local.cancel(id);
    Ok(())
}

fn broadcast(subscribers: &Mutex<Vec<Channel<LocalDownloadEvent>>>, event: LocalDownloadEvent) {
    let list = lock(subscribers);
    for channel in list.iter() {
        let _ = channel.send(event.clone());
    }
}

fn http_client() -> Result<reqwest::Client> {
    beep_proto::https::client()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .build()
        .map_err(|e| DesktopError::Other(e.into()))
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DownloadFailure {
    Cancelled,
    Failed(String),
}

pub(crate) struct VerifiedDownload<'a> {
    pub url: &'a str,
    pub expected_bytes: u64,
    pub expected_sha256: &'a str,
    pub dest_dir: &'a Path,
    pub filename: &'a str,
}

pub(crate) async fn download_verified(
    client: &reqwest::Client,
    spec: VerifiedDownload<'_>,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(u64, u64),
) -> std::result::Result<(), DownloadFailure> {
    let VerifiedDownload {
        url,
        expected_bytes,
        expected_sha256,
        dest_dir,
        filename,
    } = spec;
    tokio::fs::create_dir_all(dest_dir)
        .await
        .map_err(|e| DownloadFailure::Failed(format!("Could not create models folder: {e}")))?;

    let part_path = dest_dir.join(format!("{filename}.part"));
    let final_path = dest_dir.join(filename);
    let receipt_path = own_receipt_path(dest_dir, filename);

    let result = async {
        if cancel.load(Ordering::Relaxed) {
            return Err(DownloadFailure::Cancelled);
        }

        let mut response = client
            .get(url)
            .send()
            .await
            .map_err(|e| DownloadFailure::Failed(format!("Model download failed: {e}")))?;
        if !response.status().is_success() {
            return Err(DownloadFailure::Failed(format!(
                "Model download failed with HTTP {}.",
                response.status().as_u16()
            )));
        }

        let mut file = tokio::fs::File::create(&part_path)
            .await
            .map_err(|e| DownloadFailure::Failed(format!("Could not write model file: {e}")))?;
        let mut hasher = Sha256::new();
        let mut written: u64 = 0;
        let mut reported_bytes: u64 = 0;
        let mut reported_at = Instant::now();

        on_progress(0, expected_bytes);

        while let Some(chunk) = response.chunk().await.map_err(|e| {
            DownloadFailure::Failed(format!("Connection dropped during download: {e}"))
        })? {
            if cancel.load(Ordering::Relaxed) {
                return Err(DownloadFailure::Cancelled);
            }

            let next = written.saturating_add(chunk.len() as u64);
            if next > expected_bytes {
                return Err(DownloadFailure::Failed(
                    "Downloaded model exceeded expected size.".into(),
                ));
            }

            file.write_all(&chunk)
                .await
                .map_err(|e| DownloadFailure::Failed(format!("Could not write model file: {e}")))?;
            hasher.update(&chunk);
            written = next;

            if written.saturating_sub(reported_bytes) >= PROGRESS_STEP_BYTES
                || reported_at.elapsed() >= PROGRESS_INTERVAL
            {
                reported_bytes = written;
                reported_at = Instant::now();
                on_progress(written, expected_bytes);
            }
        }

        if cancel.load(Ordering::Relaxed) {
            return Err(DownloadFailure::Cancelled);
        }

        if written != expected_bytes {
            return Err(DownloadFailure::Failed(format!(
                "Model download ended early ({written} of {expected_bytes} bytes)."
            )));
        }

        let digest = hex::encode(hasher.finalize());
        if !digest.eq_ignore_ascii_case(expected_sha256) {
            return Err(DownloadFailure::Failed(
                "Downloaded model failed SHA-256 verification.".into(),
            ));
        }

        file.flush()
            .await
            .map_err(|e| DownloadFailure::Failed(format!("Could not flush model file: {e}")))?;
        file.sync_all()
            .await
            .map_err(|e| DownloadFailure::Failed(format!("Could not sync model file: {e}")))?;
        drop(file);

        tokio::fs::rename(&part_path, &final_path)
            .await
            .map_err(|e| DownloadFailure::Failed(format!("Could not finalize model file: {e}")))?;
        if let Ok(stamp) = file_stamp(&final_path, expected_sha256) {
            let _ = tokio::fs::write(&receipt_path, stamp).await;
        }
        on_progress(expected_bytes, expected_bytes);
        Ok(())
    }
    .await;

    if result.is_err() {
        let _ = tokio::fs::remove_file(&part_path).await;
    }

    result
}

/// Returns a verified on-disk GGUF path for `model` in Beep's own folder or an
/// existing Hex installation.
pub(crate) fn installed_model_path(paths: &ModelPaths, model: LocalModelId) -> Option<PathBuf> {
    let own = paths.own_dir.join(model.filename());
    let own_receipt = own_receipt_path(&paths.own_dir, model.filename());
    if verify_file_with_receipt(&own, &own_receipt, model.bytes(), model.sha256()) {
        return Some(own);
    }

    let hex_file = paths.hex_dir.as_ref()?.join(model.filename());
    let hex_receipt = paths
        .own_dir
        .join(format!("{}.hex.sha256", model.filename()));
    if verify_file_with_receipt(&hex_file, &hex_receipt, model.bytes(), model.sha256()) {
        return Some(hex_file);
    }

    None
}

fn own_receipt_path(dir: &Path, filename: &str) -> PathBuf {
    dir.join(format!("{filename}.sha256"))
}

fn verify_file_with_receipt(
    file: &Path,
    receipt: &Path,
    expected_bytes: u64,
    expected_sha256: &str,
) -> bool {
    let Ok(meta) = fs::metadata(file) else {
        return false;
    };
    if !meta.is_file() || meta.len() != expected_bytes {
        return false;
    }

    let Ok(expected_stamp) = stamp_from_meta(file, &meta, expected_sha256) else {
        return false;
    };
    if let Ok(saved) = fs::read_to_string(receipt)
        && saved.trim() == expected_stamp
    {
        return true;
    }

    let Ok(actual_sha256) = sha256_file(file) else {
        return false;
    };
    if !actual_sha256.eq_ignore_ascii_case(expected_sha256) {
        return false;
    }

    if let Some(parent) = receipt.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(receipt, expected_stamp);
    true
}

fn file_stamp(file: &Path, sha256: &str) -> std::io::Result<String> {
    let meta = fs::metadata(file)?;
    stamp_from_meta(file, &meta, sha256)
}

fn stamp_from_meta(file: &Path, meta: &fs::Metadata, sha256: &str) -> std::io::Result<String> {
    let mtime = meta
        .modified()
        .unwrap_or(SystemTime::UNIX_EPOCH)
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    Ok(format!(
        "{}:{}:{}:{}",
        file.to_string_lossy(),
        meta.len(),
        mtime,
        sha256.to_ascii_lowercase()
    ))
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut buf = vec![0u8; 256 * 1024];
    let mut hasher = Sha256::new();
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub(crate) async fn run_local(
    local: LocalDictation,
    model_id: LocalModelId,
    model_path: PathBuf,
    mut audio: mpsc::Receiver<AudioCommand>,
    emit: &mut (impl FnMut(DictationEvent) + Send),
) -> std::result::Result<bool, String> {
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let worker = tokio::task::spawn_blocking(move || {
        stream_blocking(&local, model_id, &model_path, &mut audio, |event| {
            let _ = event_tx.send(event);
        })
    });

    while let Some(event) = event_rx.recv().await {
        emit(event);
    }

    worker
        .await
        .map_err(|e| format!("Local dictation stopped unexpectedly: {e}"))?
}

fn stream_blocking(
    local: &LocalDictation,
    model_id: LocalModelId,
    model_path: &Path,
    audio: &mut mpsc::Receiver<AudioCommand>,
    mut emit: impl FnMut(DictationEvent),
) -> std::result::Result<bool, String> {
    let model = local.load_or_cached(model_id, model_path)?;
    let mut session = model
        .session()
        .map_err(|e| format!("Could not start {}: {e}", model_id.display_name()))?;
    let run_options = transcribe_cpp::RunOptions {
        language: model_id.language().map(str::to_owned),
        ..Default::default()
    };
    let mut stream = session
        .stream(&run_options, &transcribe_cpp::StreamOptions::default())
        .map_err(|e| format!("Could not start {}: {e}", model_id.display_name()))?;
    emit(DictationEvent::Ready);

    let mut samples = Vec::new();
    let mut sent: usize = 0;
    let mut stopped = false;

    while !stopped {
        let Some(first) = next_audio(audio.blocking_recv(), &mut sent) else {
            break;
        };

        samples.clear();
        pcm16_to_f32(&first, &mut samples);
        let mut batch_bytes = first.len();

        while batch_bytes < MAX_COALESCED_PCM_BYTES {
            match audio.try_recv() {
                Ok(command) => match next_audio(Some(command), &mut sent) {
                    Some(more) => {
                        batch_bytes = batch_bytes.saturating_add(more.len());
                        pcm16_to_f32(&more, &mut samples);
                    }
                    None => {
                        stopped = true;
                        break;
                    }
                },
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    stopped = true;
                    break;
                }
            }
        }

        if samples.is_empty() {
            continue;
        }

        let update = stream
            .feed(&samples)
            .map_err(|e| format!("Local transcription failed: {e}"))?;
        if update.result_changed {
            let text = stream.text();
            let (committed, partial) = split_stream_words(&text.committed, &text.tentative);
            emit(DictationEvent::Transcript { committed, partial });
        }
    }

    emit(DictationEvent::Thinking);
    stream
        .finalize()
        .map_err(|e| format!("Local transcription failed: {e}"))?;
    let text = stream.text();
    let combined = text.display();
    let final_text = if combined.trim().is_empty() {
        text.full.trim()
    } else {
        combined.trim()
    };

    if !final_text.is_empty() {
        emit(DictationEvent::Transcript {
            committed: final_text.to_owned(),
            partial: String::new(),
        });
    }

    Ok(final_text.is_empty())
}

pub(crate) fn pcm16_to_f32(pcm: &[u8], out: &mut Vec<f32>) {
    out.reserve(pcm.len() / 2);
    for pair in pcm.as_chunks::<2>().0 {
        out.push(f32::from(i16::from_le_bytes(*pair)) / 32_768.0);
    }
}

/// Splits `committed` and `tentative` on whole-word boundaries so subword
/// commits never introduce spaces inside an unfinished word.
pub(crate) fn split_stream_words(committed: &str, tentative: &str) -> (String, String) {
    if tentative.is_empty() {
        return (committed.trim().to_owned(), String::new());
    }
    if committed.is_empty() {
        return (String::new(), tentative.trim().to_owned());
    }
    if committed.ends_with(char::is_whitespace) || tentative.starts_with(char::is_whitespace) {
        return (committed.trim().to_owned(), tentative.trim().to_owned());
    }

    if let Some(split_at) = committed.rfind(char::is_whitespace) {
        let head = committed[..split_at].trim().to_owned();
        let tail = format!("{}{}", &committed[split_at..], tentative);
        (head, tail.trim().to_owned())
    } else {
        (
            String::new(),
            format!("{committed}{tentative}").trim().to_owned(),
        )
    }
}
