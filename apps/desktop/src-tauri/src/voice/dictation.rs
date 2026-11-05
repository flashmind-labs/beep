//! Dictation: hold Option, speak, let go, and the words are typed where the cursor is. It needs
//! no server. The microphone opens as Option goes down, so the first word is heard, and the clip
//! goes as one WAV to the transcription model picked in Assistant. Nothing is sent or read
//! aloud, and the sound stays in memory.

use super::{
    Voice,
    audio::{Audio, MicChunk, mic_allowed},
    event::LiveEvent,
    hotkey::{self, HOLD, HoldStep},
    orb,
    session::{MIC_DENIED, OpenFailure, UNAVAILABLE, open_audio},
    typing,
};
use crate::{
    fleet::projects::desk,
    machines::desktop::{permissions, request},
};
use beep_dictation::{
    DictationError, Length, Provider, RATE, Settings, is_full, judge, max_bytes, transcribe,
    wrap_pcm16,
};
use beep_fleet::Session as MeshSession;
use beep_proto::{ApiProvider, node::desktop::Permission};
use std::{
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};
use tokio::{
    sync::{mpsc, watch},
    task::AbortHandle,
    time::{sleep, timeout},
};

/// How long the orb shows a message before it goes.
const MESSAGE_FOR: Duration = Duration::from_millis(2500);
/// How long to wait for the mesh to hand over a key.
const KEY_WAIT: Duration = Duration::from_secs(5);
const NO_ACCESSIBILITY: &str =
    "Beep cannot type. Allow it in System Settings, Privacy & Security, Accessibility.";
const NOTHING_HEARD: &str = "Heard nothing";
const CANNOT_TYPE: &str = "Could not type the words";

/// The clip being recorded for one press of Option.
pub(super) struct Recording {
    id: u64,
    press: u64,
    /// 16 kHz mono 16-bit PCM, at most `max_bytes`.
    pcm: Vec<u8>,
    /// Option stayed down alone for `HOLD`: it is a dictation, and the orb shows.
    confirmed: bool,
    /// Keeps the engine open until the recording is dropped.
    _audio: watch::Receiver<Option<Arc<Audio>>>,
    tasks: Vec<AbortHandle>,
}

impl Drop for Recording {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// The API provider whose saved key dictation with `provider` uses.
pub(crate) fn api_provider(provider: Provider) -> ApiProvider {
    match provider {
        Provider::OpenRouter => ApiProvider::OpenRouter,
        Provider::OpenAi => ApiProvider::OpenAi,
        Provider::Mistral => ApiProvider::Mistral,
    }
}

impl Voice {
    /// The picked provider and model, or off.
    pub(crate) fn dictation_settings(&self) -> Settings {
        self.dictation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn set_dictation_settings(&self, settings: Settings) {
        *self
            .dictation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = settings;
    }

    /// What Option does with no server: a hold dictates, when it is on.
    pub(super) fn dictate_hold(self: &Arc<Self>, step: HoldStep) {
        match step {
            HoldStep::Wait(press) => {
                if self.dictation_settings().chosen().is_none() {
                    return;
                }
                // Asking for the microphone waits for a hold that lasts.
                if mic_allowed() {
                    self.record(press);
                }
                let me = self.clone();
                tauri::async_runtime::spawn(async move {
                    sleep(HOLD).await;
                    me.on_dictation_timer(press);
                });
            }
            HoldStep::Send => self.dictate_release(),
            HoldStep::Cancel => self.drop_recording(None),
            HoldStep::Start | HoldStep::Single | HoldStep::Tap(_) | HoldStep::Nothing => {}
        }
    }

    /// `HOLD` after Option went down: still held alone, it is a dictation.
    fn on_dictation_timer(self: &Arc<Self>, press: u64) {
        let step = self
            .state()
            .hold
            .on_timer(press, hotkey::since_press(), Instant::now());
        if step != HoldStep::Start {
            return self.drop_recording(Some(press));
        }
        if !permissions().accessibility {
            // Adds Beep to the list and opens its page; the words need it to be typed.
            tauri::async_runtime::spawn_blocking(|| request(Permission::Accessibility));
            self.drop_recording(Some(press));
            return self.message(NO_ACCESSIBILITY);
        }
        if self.state().dictation.is_none() {
            self.record(press);
        }
        let shown = {
            let mut state = self.state();
            state
                .dictation
                .as_mut()
                .filter(|r| r.press == press)
                .map(|r| r.confirmed = true)
                .is_some()
        };
        if shown {
            self.show_orb("holding");
        }
    }

    /// Opens the microphone for press `press` and keeps what it hears.
    fn record(self: &Arc<Self>, press: u64) {
        let id = self.recordings.fetch_add(1, Ordering::Relaxed) + 1;
        let opening = open_audio(true);
        let me = self.clone();
        let collector = tauri::async_runtime::spawn(me.collect(id, opening.heard));
        let me = self.clone();
        let watcher = tauri::async_runtime::spawn(async move {
            match opening.failed.await {
                Ok(OpenFailure::Denied(why)) => me.recording_failed(id, MIC_DENIED, &why),
                Ok(OpenFailure::Other(why)) => me.recording_failed(id, UNAVAILABLE, &why),
                // The engine opened.
                Err(_) => {}
            }
        });
        let recording = Recording {
            id,
            press,
            pcm: Vec::new(),
            confirmed: false,
            _audio: opening.audio,
            tasks: vec![
                collector.inner().abort_handle(),
                watcher.inner().abort_handle(),
            ],
        };
        // A press with a recording already open keeps the first.
        let mut state = self.state();
        if state.dictation.is_none() {
            state.dictation = Some(recording);
        }
    }

    /// Keeps the microphone's sound for recording `id`, and cuts the clip when it is full.
    async fn collect(self: Arc<Self>, id: u64, mut heard: mpsc::Receiver<MicChunk>) {
        while let Some(chunk) = heard.recv().await {
            let (shown, full) = {
                let mut state = self.state();
                let Some(r) = state.dictation.as_mut().filter(|r| r.id == id) else {
                    return;
                };
                let room = max_bytes().saturating_sub(r.pcm.len());
                let take = chunk.pcm.len().min(room);
                r.pcm.extend_from_slice(&chunk.pcm[..take]);
                (r.confirmed, is_full(r.pcm.len()))
            };
            if shown {
                self.emit_orb(LiveEvent::Level { level: chunk.level });
            }
            if full {
                return self.dictate_release();
            }
        }
    }

    fn recording_failed(self: &Arc<Self>, id: u64, message: &str, why: &str) {
        log::warn!("dictation could not listen: {why}");
        let wanted = self
            .state()
            .dictation
            .take_if(|r| r.id == id)
            .is_some_and(|r| r.confirmed);
        // A press that was only Option for something else says nothing.
        if wanted {
            self.message(message);
        }
    }

    /// Option came up after a dictation: sends the clip, unless it was a slip.
    fn dictate_release(self: &Arc<Self>) {
        let Some(mut recording) = self.state().dictation.take() else {
            return;
        };
        if !recording.confirmed {
            return;
        }
        let pcm = std::mem::take(&mut recording.pcm);
        if judge(pcm.len()) == Length::TooShort {
            return self.hide_orb();
        }
        self.show_orb("thinking");
        let me = self.clone();
        tauri::async_runtime::spawn(async move { me.finish(pcm).await });
    }

    /// Drops the recording for `press`, or for any press, and hides what it showed.
    fn drop_recording(self: &Arc<Self>, press: Option<u64>) {
        let dropped = self
            .state()
            .dictation
            .take_if(|r| press.is_none_or(|p| r.press == p));
        if dropped.is_some_and(|r| r.confirmed) {
            self.hide_orb();
        }
    }

    /// Esc, or the orb's x, while dictating. True when it was dictation that stopped.
    pub(super) fn cancel_dictation(self: &Arc<Self>) -> bool {
        let dropped = self.state().dictation.take();
        let was = dropped.is_some_and(|r| r.confirmed);
        if was {
            self.hide_orb();
        }
        was
    }

    /// Transcribes the clip and types the words.
    async fn finish(self: Arc<Self>, pcm: Vec<u8>) {
        let words = match self.transcribe_clip(&pcm).await {
            Ok(words) => words,
            Err(e) => return self.message(&e.to_string()),
        };
        if words.is_empty() {
            return self.message(NOTHING_HEARD);
        }
        match tauri::async_runtime::spawn_blocking(move || typing::type_text(&words)).await {
            Ok(Ok(())) => self.hide_orb(),
            Ok(Err(e)) => {
                log::warn!("typing dictated words: {e}");
                self.message(CANNOT_TYPE);
            }
            Err(e) => {
                log::warn!("typing dictated words: {e}");
                self.message(CANNOT_TYPE);
            }
        }
    }

    async fn transcribe_clip(&self, pcm: &[u8]) -> Result<String, DictationError> {
        let chosen = self
            .dictation_settings()
            .chosen()
            .ok_or(DictationError::Off)?;
        let fleet = self.app.state::<Arc<MeshSession>>();
        let key = async {
            desk(&fleet)
                .await
                .ok()?
                .api_key(api_provider(chosen.provider))
        };
        let key = timeout(KEY_WAIT, key)
            .await
            .map_err(|_| DictationError::Keys)?
            .ok_or(DictationError::NoKey(chosen.provider.name()))?;
        transcribe(
            chosen.provider,
            None,
            &key,
            &chosen.model,
            wrap_pcm16(pcm, RATE),
        )
        .await
    }

    // The orb, as dictation uses it: only the orb hears these, never the bar.

    fn emit_orb(&self, event: LiveEvent) {
        if let Err(e) = self.app.emit_to(orb::LABEL, "live", &event) {
            log::debug!("telling the orb about dictation: {e}");
        }
    }

    fn show_orb(&self, phase: &'static str) {
        self.dictations.fetch_add(1, Ordering::Relaxed);
        if let Err(e) = orb::show(&self.app) {
            log::warn!("showing the orb: {e}");
        }
        self.emit_orb(LiveEvent::View {
            phase,
            words: String::new(),
            line: None,
            prev: None,
            step: None,
            speaking: false,
            pause_sends: false,
            session: None,
            title: None,
            hold_only: true,
            finishing: false,
            sheet: false,
            dictating: true,
        });
    }

    /// Takes the orb away, unless a notice card is up on it.
    fn hide_orb(&self) {
        self.emit_orb(LiveEvent::Left {
            draft: None,
            session: None,
            quiet: false,
            failure: None,
        });
        if self.state().card.is_none() {
            orb::hide(&self.app);
        }
    }

    /// Tells the user something in the orb, then takes it away, unless something newer showed.
    fn message(self: &Arc<Self>, text: &str) {
        let shown = self.dictations.fetch_add(1, Ordering::Relaxed) + 1;
        if let Err(e) = orb::show(&self.app) {
            log::warn!("showing the orb: {e}");
        }
        self.emit_orb(LiveEvent::Toast { text: text.into() });
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            sleep(MESSAGE_FOR).await;
            if me.dictations.load(Ordering::Relaxed) == shown {
                me.hide_orb();
            }
        });
    }
}
