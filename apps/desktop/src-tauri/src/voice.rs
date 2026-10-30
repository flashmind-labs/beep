//! Live voice. Three taps of Option, a held Option, or a voice note start
//! live mode: the bar shows an orb, the words as they are said, and Beep's
//! answer read aloud sentence by sentence, then it listens again.
//!
//! `beep_live::Live` decides; this runs its effects. The microphone and the
//! player share one engine with echo cancellation (`audio`), the words
//! come from the server's live transcription, and each sentence of the
//! reply is spoken through `Say` as it streams in.

use crate::{
    audio::{Audio, MicChunk, MicDenied, Played},
    bar,
    beep::{Beep, Pick, RunNews, Sent},
    hotkey::{self, HOLD, HoldStep, Input as Key, TapStep, Taps, WINDOW},
    news::{self, Heard},
    presence::Presence,
};
use anyhow::Context;
use beep_live::{Effect, EndMode, Filler, Fillers, Input, Leaving, Live, Phase, Reading};
use beep_proto::{Filler as FillerLine, Notice, Say, SpeakNotice, Transcription, VoiceNote};
use serde::Serialize;
use serde_bytes::ByteBuf;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, ipc::Channel};
use tokio::{
    sync::{mpsc, watch},
    task::AbortHandle,
    time::{interval, sleep},
};
use uuid::Uuid;

/// How often live mode checks pauses and moves the orb.
const TICK: Duration = Duration::from_millis(50);
/// How often a run is checked for a filler.
const FILLER_CHECK: Duration = Duration::from_millis(250);
/// Microphone stretches waiting to go up, about six seconds' worth: enough
/// to cover opening the transcription.
const MIC_CHUNKS: usize = 64;
/// Sentences waiting for their audio.
const LINES: usize = 64;
/// Spoken sentences waiting to play.
const READY: usize = 2;
/// Loudness above which the microphone counts as hearing someone.
const SOUND_LEVEL: f32 = 0.12;
/// Failed transcriptions in a row before live mode gives up.
const STT_TRIES: u32 = 3;
/// Wait before opening a transcription again after one failed.
const STT_RETRY: Duration = Duration::from_secs(1);
/// How long a passive bar stays up after live mode ends with nothing kept.
const LINGER: Duration = Duration::from_secs(3);
const UNAVAILABLE: &str = "Voice is not available right now";
const MIC_DENIED: &str =
    "Beep cannot hear you. Allow it in System Settings, Privacy & Security, Microphone.";

/// What the bar hears about live mode, as the `live` event.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum LiveEvent {
    View {
        phase: &'static str,
        /// What the user said so far this turn.
        words: String,
        /// Beep's sentence on screen, and the one before it.
        line: Option<String>,
        prev: Option<String>,
        /// The tool step running.
        step: Option<String>,
        /// Whether Beep's voice is playing, filler or answer.
        speaking: bool,
        pause_sends: bool,
        session: Option<String>,
        /// The voice note's title, when one started live mode.
        title: Option<String>,
    },
    Level {
        level: f32,
    },
    Toast {
        text: &'static str,
    },
    /// A pause sends after `ms`, or no longer will.
    Soon {
        ms: Option<u64>,
    },
    /// A spoken message went to `session`.
    Sent {
        session: String,
        text: String,
    },
    Left {
        draft: Option<String>,
        session: Option<String>,
        quiet: bool,
        failure: Option<String>,
    },
}

/// How live mode starts.
enum Start {
    Tap,
    Hold,
    Note { note: VoiceNote, title: String },
}

/// Audio playing now, for the orb.
struct Playing {
    since: Instant,
    played: Played,
}

/// One live session, from start to leaving.
struct Session {
    id: u64,
    live: Live,
    chat: Option<Uuid>,
    title: Option<String>,
    /// The engine once it opened.
    audio: watch::Receiver<Option<Arc<Audio>>>,
    /// Microphone sound for the transcription that runs.
    stt: Option<mpsc::Sender<Vec<u8>>>,
    /// Counts transcriptions, so words from an ended one are dropped.
    stt_run: u64,
    stt_failures: u32,
    failure: Option<String>,
    /// Reading a reply or a note aloud.
    reader: Reader,
    playing: Option<Playing>,
    mic_level: f32,
    /// What the user sent last, so fillers are in its language.
    said: String,
    /// The last reply's sentences, for "repeat that".
    last_reply: Vec<String>,
    line: Option<String>,
    prev: Option<String>,
    step: Option<String>,
    tasks: Vec<AbortHandle>,
}

impl Session {
    /// Stops anything being read out.
    fn hush(&mut self) {
        self.reader.stop();
        if let Some(audio) = self.audio.borrow().as_ref() {
            audio.stop_playing();
        }
        self.playing = None;
        self.step = None;
    }
}

/// The task reading aloud. It is started and stopped only through the
/// session, under the state lock, so a hush cannot fall between spawning a
/// reader and keeping its handle.
#[derive(Default)]
struct Reader(Option<AbortHandle>);

impl Reader {
    /// Stops the reader there is and spawns `task` in its place.
    fn start<F>(&mut self, task: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.stop();
        self.0 = Some(tauri::async_runtime::spawn(task).inner().abort_handle());
    }

    fn stop(&mut self) {
        if let Some(reader) = self.0.take() {
            reader.abort();
        }
    }
}

#[derive(Default)]
struct State {
    taps: Taps,
    hold: hotkey::Hold,
    live: Option<Session>,
}

/// A sentence for the reader, and whether it is a filler.
enum Line {
    Answer(String),
    Filler(Filler),
}

/// A sentence with its audio, ready to play.
struct Ready {
    text: String,
    audio: Vec<u8>,
    filler: bool,
}

/// Fillers and how many lines wait to be spoken, shared by the reader's
/// parts.
struct Voiced {
    fillers: Fillers,
    queued: u32,
}

pub struct Voice {
    app: AppHandle,
    beep: Arc<Beep>,
    state: Mutex<State>,
    /// Counts live sessions.
    lives: AtomicU64,
    /// Counts audio files, so each sentence has its own.
    files: AtomicU64,
}

impl Voice {
    pub fn new(app: AppHandle, beep: Arc<Beep>) -> Arc<Self> {
        Arc::new(Self {
            app,
            beep,
            state: Mutex::new(State::default()),
            lives: AtomicU64::new(0),
            files: AtomicU64::new(0),
        })
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn emit(&self, event: LiveEvent) {
        if let Err(e) = self.app.emit_to(bar::LABEL, "live", event) {
            log::debug!("telling the bar about live mode: {e}");
        }
    }

    fn end_mode(&self) -> EndMode {
        if self.beep.send_on_pause() {
            EndMode::Pause
        } else {
            EndMode::Word
        }
    }

    /// Whether live mode is on.
    pub fn is_live(&self) -> bool {
        self.state().live.is_some()
    }

    /// Whether `notice` should be spoken instead of shown as a banner: voice
    /// notes are on, the bar is closed, live mode is off, and the user is
    /// here and not on a call.
    pub fn wants(&self, notice: &Notice) -> bool {
        self.beep.voice_notes()
            && notice.approval.is_none()
            && notice.app.is_none()
            && !(notice.title.trim().is_empty() && notice.body.trim().is_empty())
            && !bar::is_open(&self.app)
            && !self.is_live()
            && Presence::now().can_speak()
    }

    /// Reads `notice` out in live mode, or shows its banner when that fails.
    pub async fn speak_notice(self: Arc<Self>, notice: Notice) {
        let note = self
            .beep
            .call(SpeakNotice {
                title: notice.title.clone(),
                body: notice.body.clone(),
            })
            .await;
        let note = match note {
            Ok(note) => note,
            Err(e) => {
                log::warn!("speaking a notice: {e}");
                crate::notify(&self.app, &notice);
                return;
            }
        };
        let chat = Uuid::parse_str(&notice.session_id).ok();
        if self.is_live() || bar::is_open(&self.app) {
            crate::notify(&self.app, &notice);
            return;
        }
        if let Some(chat) = chat {
            self.beep.set_session(Some(chat.to_string()));
        }
        let title = notice.title.clone();
        self.start(Start::Note { note, title }, chat);
    }

    /// Live mode in chat `session`, from its Continue live button.
    pub fn continue_live(self: &Arc<Self>, session: Uuid) {
        if self.is_live() {
            return;
        }
        self.beep.set_session(Some(session.to_string()));
        self.start(Start::Tap, Some(session));
    }

    /// A tap from the bar, such as a click on the orb.
    pub fn tap(self: &Arc<Self>) {
        self.input_now(Input::Tap);
    }

    /// Leaves live mode, keeping what was said as a draft.
    pub fn leave_live(self: &Arc<Self>) {
        self.input_now(Input::Escape);
    }

    /// The pause setting changed.
    pub fn set_end_mode(&self) {
        let mode = self.end_mode();
        if let Some(s) = self.state().live.as_mut() {
            s.live.set_mode(mode);
        }
    }

    /// The chat open in the bar, when the user has it up.
    fn open_chat(&self) -> Option<Uuid> {
        if !bar::is_open(&self.app) || bar::is_passive() {
            return None;
        }
        self.beep.session().and_then(|s| Uuid::parse_str(&s).ok())
    }

    fn start(self: &Arc<Self>, how: Start, chat: Option<Uuid>) {
        let now = Instant::now();
        let mode = self.end_mode();
        let id = self.lives.fetch_add(1, Ordering::Relaxed) + 1;
        let (mic, heard) = mpsc::channel(MIC_CHUNKS);
        let (opened, audio) = watch::channel(None);
        let (live, effects, note) = match how {
            Start::Tap => {
                let (live, effects) = Live::listening(mode, now);
                (live, effects, None)
            }
            Start::Hold => {
                let (live, effects) = Live::holding(mode, now);
                (live, effects, None)
            }
            Start::Note { note, title } => {
                let (live, effects) = Live::speaking(mode, now);
                (live, effects, Some((note, title)))
            }
        };
        {
            let mut state = self.state();
            if state.live.is_some() {
                return;
            }
            let tasks = vec![
                tauri::async_runtime::spawn(self.clone().pump(id, heard))
                    .inner()
                    .abort_handle(),
                tauri::async_runtime::spawn(self.clone().tick(id))
                    .inner()
                    .abort_handle(),
            ];
            state.live = Some(Session {
                id,
                live,
                chat,
                title: note.as_ref().map(|(_, title)| title.clone()),
                audio,
                stt: None,
                stt_run: 0,
                stt_failures: 0,
                failure: None,
                reader: Reader::default(),
                playing: None,
                mic_level: 0.0,
                said: String::new(),
                last_reply: Vec::new(),
                line: None,
                prev: None,
                step: None,
                tasks,
            });
        }
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            let open = tauri::async_runtime::spawn_blocking(move || Audio::open(mic)).await;
            match open {
                Ok(Ok(audio)) => {
                    // Nobody waiting means live mode already ended.
                    let _ = opened.send(Some(Arc::new(audio)));
                }
                Ok(Err(e)) if e.is::<MicDenied>() => me.fail(id, MIC_DENIED, format!("{e:#}")),
                Ok(Err(e)) => me.unavailable(id, format!("{e:#}")),
                Err(e) => me.unavailable(id, format!("the audio did not start: {e}")),
            }
        });
        if !bar::is_open(&self.app) {
            bar::show_passive(&self.app);
        }
        self.watch_escape();
        if let Some((note, _)) = note {
            self.read_note(id, note);
        }
        self.apply(id, effects);
        self.show(id);
    }

    fn watch_escape(self: &Arc<Self>) {
        let me = self.clone();
        let watched = self.app.run_on_main_thread(move || {
            hotkey::watch_escape(move || me.leave_live());
        });
        if let Err(e) = watched {
            log::warn!("watching Escape: {e}");
        }
    }

    fn unwatch_escape(&self) {
        if let Err(e) = self.app.run_on_main_thread(hotkey::unwatch_escape) {
            log::warn!("no longer watching Escape: {e}");
        }
    }

    /// Hands `input` to the live session running now.
    fn input_now(self: &Arc<Self>, input: Input) {
        let id = self.state().live.as_ref().map(|s| s.id);
        if let Some(id) = id {
            self.input(id, input);
        }
    }

    /// Hands `input` to live session `id` and runs what it asks for.
    fn input(self: &Arc<Self>, id: u64, input: Input) {
        let effects = {
            let mut state = self.state();
            let Some(s) = state.live.as_mut().filter(|s| s.id == id) else {
                return;
            };
            let before = s.live.phase();
            let effects = s.live.input(input, Instant::now());
            let after = s.live.phase();
            // Back to listening from a reply: nothing more is read out.
            if matches!(after, Phase::Listening | Phase::Holding)
                && matches!(before, Phase::Thinking | Phase::Speaking)
            {
                s.hush();
            }
            effects
        };
        if !effects.is_empty() {
            self.apply(id, effects);
            self.show(id);
        }
    }

    fn apply(self: &Arc<Self>, id: u64, effects: Vec<Effect>) {
        for effect in effects {
            log::debug!("live {id}: {}", effect.name());
            match effect {
                Effect::Transcribe => self.transcribe(id),
                Effect::EndTranscription => self.end_transcription(id),
                Effect::Send { text, new_chat } => self.send(id, text, new_chat),
                // The reader already plays the reply as it comes.
                Effect::Speak => {}
                Effect::Repeat => self.repeat(id),
                Effect::StopSpeaking => {
                    if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
                        s.hush();
                    }
                }
                Effect::Leave { draft, why } => self.leave(id, draft, why),
                Effect::Toast(toast) => self.emit(LiveEvent::Toast { text: toast.text() }),
                Effect::Soon(after) => self.emit(LiveEvent::Soon {
                    ms: after.map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
                }),
                Effect::Show => {}
            }
        }
    }

    fn show(&self, id: u64) {
        let pause_sends = self.beep.send_on_pause();
        let view = {
            let state = self.state();
            let Some(s) = state.live.as_ref().filter(|s| s.id == id) else {
                return;
            };
            LiveEvent::View {
                phase: s.live.phase().as_str(),
                words: s.live.text(),
                line: s.line.clone(),
                prev: s.prev.clone(),
                step: s.step.clone(),
                speaking: s.playing.is_some(),
                pause_sends,
                session: s.chat.map(|c| c.to_string()),
                title: s.title.clone(),
            }
        };
        self.emit(view);
    }

    /// Forwards the microphone: to the transcription while one runs, as
    /// sound for pauses, and as the orb's level.
    async fn pump(self: Arc<Self>, id: u64, mut heard: mpsc::Receiver<MicChunk>) {
        while let Some(chunk) = heard.recv().await {
            {
                let mut state = self.state();
                let Some(s) = state.live.as_mut().filter(|s| s.id == id) else {
                    return;
                };
                s.mic_level = chunk.level;
                if let Some(stt) = s.stt.as_ref() {
                    // A full queue drops a stretch rather than falling behind.
                    let _ = stt.try_send(chunk.pcm);
                }
            }
            if chunk.level >= SOUND_LEVEL {
                self.input(id, Input::Sound);
            }
        }
    }

    /// Checks pauses and sends the orb its level.
    async fn tick(self: Arc<Self>, id: u64) {
        let mut every = interval(TICK);
        loop {
            every.tick().await;
            self.input(id, Input::Tick);
            let level = {
                let mut state = self.state();
                let Some(s) = state.live.as_mut().filter(|s| s.id == id) else {
                    return;
                };
                match s.playing.as_ref() {
                    Some(p) => p.played.level_at(p.since.elapsed()),
                    None => {
                        // Fades between microphone stretches.
                        s.mic_level *= 0.85;
                        s.mic_level
                    }
                }
            };
            self.emit(LiveEvent::Level { level });
        }
    }

    fn transcribe(self: &Arc<Self>, id: u64) {
        let (stt, sound) = mpsc::channel(MIC_CHUNKS);
        let run = {
            let mut state = self.state();
            let Some(s) = state.live.as_mut().filter(|s| s.id == id) else {
                return;
            };
            s.stt = Some(stt);
            s.stt_run += 1;
            s.stt_run
        };
        let me = self.clone();
        tauri::async_runtime::spawn(async move { me.run_transcription(id, run, sound).await });
    }

    fn end_transcription(&self, id: u64) {
        if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
            // Dropping the sender ends the audio; the run's last words no
            // longer count.
            s.stt = None;
            s.stt_run += 1;
        }
    }

    /// Whether transcription `run` of session `id` is the one running.
    fn is_run(&self, id: u64, run: u64) -> bool {
        self.state()
            .live
            .as_ref()
            .is_some_and(|s| s.id == id && s.stt_run == run)
    }

    async fn run_transcription(
        self: Arc<Self>,
        id: u64,
        run: u64,
        mut sound: mpsc::Receiver<Vec<u8>>,
    ) {
        let (mut uplink, mut replies) = match self.beep.transcribe().await {
            Ok(opened) => opened,
            Err(e) => return self.stt_failed(id, run, e.to_string()),
        };
        let up = tauri::async_runtime::spawn(async move {
            let mut sent = 0usize;
            while let Some(pcm) = sound.recv().await {
                sent += pcm.len();
                if let Err(e) = uplink.send(&ByteBuf::from(pcm)).await {
                    log::debug!("live audio stopped going up: {e}");
                    return;
                }
            }
            log::info!("live transcription {run}: {sent} bytes of audio sent");
            if let Err(e) = uplink.finish() {
                log::debug!("ending live audio: {e}");
            }
        });
        loop {
            match replies.next().await {
                Ok(Some(Transcription::Delta { text })) => {
                    if !self.is_run(id, run) {
                        break;
                    }
                    if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
                        s.stt_failures = 0;
                    }
                    self.input(id, Input::Words(text));
                }
                Ok(Some(Transcription::Done { .. }) | None) => break,
                Ok(Some(Transcription::Unavailable {})) => {
                    self.unavailable(id, "the server has no live transcription".into());
                    break;
                }
                Err(e) => {
                    self.stt_failed(id, run, e.to_string());
                    break;
                }
            }
        }
        up.abort();
    }

    /// Opens the transcription again, unless it keeps failing.
    fn stt_failed(self: &Arc<Self>, id: u64, run: u64, message: String) {
        log::warn!("live transcription: {message}");
        let tries = {
            let mut state = self.state();
            let Some(s) = state
                .live
                .as_mut()
                .filter(|s| s.id == id && s.stt_run == run && s.stt.is_some())
            else {
                return;
            };
            s.stt_failures += 1;
            s.stt_failures
        };
        if tries >= STT_TRIES {
            return self.unavailable(id, message);
        }
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            sleep(STT_RETRY).await;
            if me.is_run(id, run) {
                me.transcribe(id);
            }
        });
    }

    /// The microphone or the transcription cannot work: leave, keeping
    /// what was said. The app keeps no log, so the reason goes on screen.
    fn unavailable(self: &Arc<Self>, id: u64, why: String) {
        self.fail(id, &format!("{UNAVAILABLE}: {why}."), why);
    }

    /// Leaves live mode, telling the user `failure`.
    fn fail(self: &Arc<Self>, id: u64, failure: &str, why: String) {
        log::warn!("live mode is unavailable: {why}");
        if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
            s.failure = Some(failure.into());
        }
        self.input(id, Input::Escape);
    }

    fn leave(&self, id: u64, draft: Option<String>, why: Leaving) {
        let session = {
            let mut state = self.state();
            if !state.live.as_ref().is_some_and(|s| s.id == id) {
                return;
            }
            state.live.take()
        };
        let Some(mut session) = session else {
            return;
        };
        session.hush();
        for task in session.tasks.drain(..) {
            task.abort();
        }
        self.unwatch_escape();
        let chat = session.chat.or_else(|| self.open_chat());
        if let Some(chat) = chat {
            self.beep.set_session(Some(chat.to_string()));
        }
        // The bar opens on the chat when there is a draft to edit or the
        // user had it up; a bar up only for live mode goes away.
        let reopen = draft.is_some() || (bar::is_open(&self.app) && !bar::is_passive());
        self.emit(LiveEvent::Left {
            draft,
            session: chat.map(|c| c.to_string()),
            quiet: why == Leaving::Quiet,
            failure: session.failure.take(),
        });
        // Dropping the session closes the microphone.
        drop(session);
        if reopen {
            bar::show(&self.app, None);
        } else {
            self.linger();
        }
    }

    /// Hides a passive bar a little after live mode ended.
    fn linger(&self) {
        let me = self.app.state::<Arc<Voice>>().inner().clone();
        tauri::async_runtime::spawn(async move {
            sleep(LINGER).await;
            if !me.is_live() && bar::is_passive() {
                bar::hide(&me.app);
            }
        });
    }

    /// Sends what was said, and reads the reply out as it streams.
    fn send(self: &Arc<Self>, id: u64, text: String, new_chat: bool) {
        let chat = {
            let mut state = self.state();
            let Some(s) = state.live.as_mut().filter(|s| s.id == id) else {
                return;
            };
            let chat = if new_chat {
                Uuid::new_v4()
            } else {
                s.chat.unwrap_or_else(Uuid::new_v4)
            };
            s.chat = Some(chat);
            s.title = None;
            text.clone_into(&mut s.said);
            s.last_reply.clear();
            s.prev = None;
            s.line = None;
            s.step = None;
            chat
        };
        self.beep.add_live_chat(chat);
        self.beep.set_session(Some(chat.to_string()));
        self.emit(LiveEvent::Sent {
            session: chat.to_string(),
            text: text.clone(),
        });
        let (news, heard) = news::channel();
        let beep = self.beep.clone();
        let said = text.clone();
        let run = tauri::async_runtime::spawn(async move {
            let sent = Sent {
                message: text,
                images: Vec::new(),
                files: Vec::new(),
                research: false,
                voice: None,
                spoken: true,
            };
            // The chat shows the run when it is opened; nothing here draws it.
            let quiet = Channel::new(|_| Ok(()));
            let answer = beep
                .post_heard(chat, sent, Pick::default(), quiet, Some(news))
                .await;
            if let Err(e) = &answer {
                log::warn!("sending a spoken message: {e}");
            }
            answer.is_ok()
        });
        let me = self.clone();
        let reader = async move {
            let voiced = Mutex::new(Voiced {
                fillers: Fillers::new(Instant::now()),
                queued: 0,
            });
            let spoke = me.read(id, Feed::Reply { heard, said }, &voiced).await;
            let sent = run.await.unwrap_or(false);
            let done = if sent {
                Input::SpeakDone
            } else {
                me.emit(LiveEvent::Toast {
                    text: "Could not send",
                });
                Input::ReplyFailed
            };
            if !spoke && sent {
                log::debug!("the reply had nothing to say aloud");
            }
            me.input(id, done);
        };
        if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
            s.reader.start(reader);
        }
    }

    /// Reads the last reply again.
    fn repeat(self: &Arc<Self>, id: u64) {
        let mut state = self.state();
        let Some(s) = state.live.as_mut().filter(|s| s.id == id) else {
            return;
        };
        s.hush();
        let lines = std::mem::take(&mut s.last_reply);
        let me = self.clone();
        s.reader.start(async move {
            let voiced = Mutex::new(Voiced {
                fillers: Fillers::new(Instant::now()),
                queued: 0,
            });
            me.read(id, Feed::Lines(lines), &voiced).await;
            me.input(id, Input::SpeakDone);
        });
    }

    /// Plays the voice note that started live mode.
    fn read_note(self: &Arc<Self>, id: u64, note: VoiceNote) {
        let me = self.clone();
        let reader = async move {
            let ready = Ready {
                text: note.text,
                audio: note.audio.data,
                filler: false,
            };
            let (tx, rx) = mpsc::channel(1);
            // The channel has room for it.
            let _ = tx.try_send(ready);
            drop(tx);
            let voiced = Mutex::new(Voiced {
                fillers: Fillers::new(Instant::now()),
                queued: 1,
            });
            let (_, ticks) = watch::channel(0);
            me.play(id, rx, ticks, &voiced, false).await;
            me.input(id, Input::SpeakDone);
        };
        if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
            s.reader.start(reader);
        }
    }

    /// Speaks `feed` line by line: getting each line's audio while the one
    /// before plays. True when any of the answer was spoken.
    async fn read(self: &Arc<Self>, id: u64, feed: Feed, voiced: &Mutex<Voiced>) -> bool {
        let (lines, todo) = mpsc::channel(LINES);
        let (ready, playable) = mpsc::channel(READY);
        let (answers, ticks) = watch::channel(0u64);
        let said = match &feed {
            Feed::Reply { said, .. } => said.clone(),
            Feed::Lines(_) => String::new(),
        };
        let fed = self.feed(id, feed, lines, voiced);
        let voiced_up = self.synthesize(todo, ready, answers, said);
        let played = self.play(id, playable, ticks, voiced, true);
        let ((), (), spoke) = tokio::join!(fed, voiced_up, played);
        spoke
    }

    /// Turns the reply into lines as it streams, with fillers in its
    /// quiet stretches.
    async fn feed(&self, id: u64, feed: Feed, lines: mpsc::Sender<Line>, voiced: &Mutex<Voiced>) {
        let queue = |line: Line| {
            let mut v = lock(voiced);
            v.queued += 1;
            v.fillers.speaking(true, Instant::now());
            line
        };
        let (mut heard, _) = match feed {
            Feed::Lines(all) => {
                for line in all {
                    if lines.send(queue(Line::Answer(line))).await.is_err() {
                        return;
                    }
                }
                return;
            }
            Feed::Reply { heard, said } => (heard, said),
        };
        let mut reading = Reading::default();
        let mut check = interval(FILLER_CHECK);
        loop {
            tokio::select! {
                news = heard.recv() => match news {
                    Some(RunNews::Text(delta)) => {
                        for line in reading.push(&delta) {
                            if lines.send(queue(Line::Answer(line))).await.is_err() {
                                return;
                            }
                        }
                    }
                    Some(RunNews::Tool(step)) => {
                        lock(voiced).fillers.tool(&step);
                        self.set_step(id, Some(step));
                    }
                    None => break,
                },
                _ = check.tick() => {
                    let due = lock(voiced).fillers.due(Instant::now());
                    if let Some(filler) = due
                        && lines.send(queue(Line::Filler(filler))).await.is_err()
                    {
                        return;
                    }
                }
            }
        }
        for line in reading.finish() {
            if lines.send(queue(Line::Answer(line))).await.is_err() {
                return;
            }
        }
    }

    /// Gets each line's audio from the server, in order.
    async fn synthesize(
        &self,
        mut todo: mpsc::Receiver<Line>,
        ready: mpsc::Sender<Ready>,
        answers: watch::Sender<u64>,
        said: String,
    ) {
        while let Some(line) = todo.recv().await {
            let (text, filler, note) = match line {
                Line::Answer(text) => {
                    let note = self.beep.call(Say { text: text.clone() }).await;
                    (text, false, note)
                }
                Line::Filler(Filler { step, still }) => {
                    let note = self
                        .beep
                        .call(FillerLine {
                            step,
                            said: said.clone(),
                            still,
                        })
                        .await;
                    let text = note.as_ref().map(|n| n.text.clone()).unwrap_or_default();
                    (text, true, note)
                }
            };
            let ready_line = match note {
                Ok(note) => Ready {
                    text,
                    audio: note.audio.data,
                    filler,
                },
                Err(e) => {
                    log::warn!("speaking a line: {e}");
                    // Still counted, so the queue empties.
                    Ready {
                        text,
                        audio: Vec::new(),
                        filler,
                    }
                }
            };
            if !filler {
                answers.send_modify(|n| *n += 1);
            }
            if ready.send(ready_line).await.is_err() {
                return;
            }
        }
    }

    /// Plays lines as they are ready. A filler stops when an answer line
    /// is ready. `reply` says the lines are an answer: the first one moves
    /// live mode to speaking.
    async fn play(
        self: &Arc<Self>,
        id: u64,
        mut playable: mpsc::Receiver<Ready>,
        mut answers: watch::Receiver<u64>,
        voiced: &Mutex<Voiced>,
        reply: bool,
    ) -> bool {
        let Some(audio) = self.audio(id).await else {
            return false;
        };
        let mut spoke = false;
        while let Some(line) = playable.recv().await {
            // An answer readied since the last line started makes a filler
            // moot.
            let fresh = answers.has_changed().unwrap_or(false);
            answers.borrow_and_update();
            let cut = line.filler && fresh;
            if !line.audio.is_empty() && !cut {
                if !line.filler && reply && !spoke {
                    self.input_reply_ready(id);
                }
                if let Err(e) = self.play_line(id, &audio, &line, &mut answers).await {
                    log::warn!("playing a line: {e:#}");
                }
                if !line.filler {
                    spoke = true;
                    if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
                        s.last_reply.push(line.text.clone());
                    }
                }
            }
            let mut v = lock(voiced);
            v.queued = v.queued.saturating_sub(1);
            if v.queued == 0 {
                v.fillers.speaking(false, Instant::now());
            }
        }
        spoke
    }

    fn input_reply_ready(self: &Arc<Self>, id: u64) {
        let thinking = self
            .state()
            .live
            .as_ref()
            .is_some_and(|s| s.id == id && s.live.phase() == Phase::Thinking);
        if thinking {
            self.input(id, Input::ReplyReady);
        }
    }

    async fn play_line(
        &self,
        id: u64,
        audio: &Audio,
        line: &Ready,
        answers: &mut watch::Receiver<u64>,
    ) -> anyhow::Result<()> {
        let path = self.line_path()?;
        tokio::fs::write(&path, &line.audio)
            .await
            .with_context(|| format!("writing {}", path.display()))?;
        let played = audio.play(&path).await;
        let removed = tokio::fs::remove_file(&path).await;
        if let Err(e) = removed {
            log::debug!("removing {}: {e}", path.display());
        }
        let (played, done) = played?;
        if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
            s.prev = s.line.take();
            s.line = Some(line.text.clone());
            s.playing = Some(Playing {
                since: Instant::now(),
                played,
            });
        }
        self.show(id);
        if line.filler {
            tokio::select! {
                _ = done => {}
                // An answer is ready: the filler gives way.
                _ = answers.changed() => audio.stop_playing(),
            }
        } else {
            // Stopped playback ends it as well.
            let _ = done.await;
        }
        if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
            s.playing = None;
        }
        self.show(id);
        Ok(())
    }

    fn line_path(&self) -> anyhow::Result<PathBuf> {
        let dir = self.app.path().app_cache_dir()?;
        std::fs::create_dir_all(&dir).with_context(|| format!("making {}", dir.display()))?;
        let n = self.files.fetch_add(1, Ordering::Relaxed);
        Ok(dir.join(format!("live-{n}.mp3")))
    }

    /// The engine of session `id`, once it opened.
    async fn audio(&self, id: u64) -> Option<Arc<Audio>> {
        let mut audio = {
            let state = self.state();
            state.live.as_ref().filter(|s| s.id == id)?.audio.clone()
        };
        let opened = audio.wait_for(Option::is_some).await.ok()?;
        opened.clone()
    }

    fn set_step(&self, id: u64, step: Option<String>) {
        if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
            s.step = step;
        }
        self.show(id);
    }

    /// Every Option change, from the hotkey monitor on the main thread.
    pub fn on_input(self: &Arc<Self>, key: Key) {
        let now = Instant::now();
        let (taps, hold) = {
            let mut state = self.state();
            let hold = state.hold.on_event(key, now);
            let taps = state.taps.on_event(key, now);
            if taps != TapStep::Nothing {
                state.hold.forget_tap();
            }
            (taps, hold)
        };
        match taps {
            TapStep::Two(run) => {
                let me = self.clone();
                tauri::async_runtime::spawn(async move {
                    sleep(WINDOW).await;
                    me.on_two_taps(run);
                });
            }
            TapStep::Three => {
                if !self.is_live() {
                    let chat = self.open_chat();
                    self.start(Start::Tap, chat);
                }
            }
            TapStep::Nothing => {}
        }
        match hold {
            HoldStep::Wait(press) => {
                let me = self.clone();
                tauri::async_runtime::spawn(async move {
                    sleep(HOLD).await;
                    me.on_hold_timer(press);
                });
            }
            HoldStep::Send => self.input_now(Input::HoldEnd),
            HoldStep::Cancel => self.input_now(Input::HoldCancel),
            HoldStep::Tap(press) => {
                let me = self.clone();
                tauri::async_runtime::spawn(async move {
                    sleep(WINDOW).await;
                    me.on_tap_timer(press);
                });
            }
            HoldStep::Start | HoldStep::Single | HoldStep::Nothing => {}
        }
    }

    /// Two taps stood: open or hide the bar. In live mode it leaves, and
    /// the bar opens on the chat.
    fn on_two_taps(self: &Arc<Self>, run: u64) {
        if !self.state().taps.on_timer(run) {
            return;
        }
        if self.is_live() {
            self.leave_live();
            if !self.is_live() {
                bar::show(&self.app, None);
            }
            return;
        }
        bar::toggle(&self.app);
    }

    /// `HOLD` after Option went down: talk while it stays down.
    fn on_hold_timer(self: &Arc<Self>, press: u64) {
        let live = self.is_live();
        let armed = live || (bar::is_open(&self.app) && !bar::is_passive());
        let step = self.state().hold.on_timer(press, armed, Instant::now());
        if step != HoldStep::Start {
            return;
        }
        if live {
            self.input_now(Input::HoldStart);
        } else {
            let chat = self.open_chat();
            self.start(Start::Hold, chat);
        }
    }

    /// A single tap stood alone: interrupt or leave live mode.
    fn on_tap_timer(self: &Arc<Self>, press: u64) {
        let step = self
            .state()
            .hold
            .on_tap_timer(press, hotkey::since_press(), Instant::now());
        if step == HoldStep::Single {
            self.input_now(Input::Tap);
        }
    }
}

/// What the reader speaks: a reply as it streams, or lines known already.
enum Feed {
    Reply { heard: Heard, said: String },
    Lines(Vec<String>),
}

fn lock(voiced: &Mutex<Voiced>) -> MutexGuard<'_, Voiced> {
    voiced.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::oneshot;

    /// A reader that says a line once `go` fires, and the receiver that
    /// hears it, or an error when the reader was stopped first.
    fn gated_reader(reader: &mut Reader) -> (oneshot::Sender<()>, oneshot::Receiver<()>) {
        let (go, gate) = oneshot::channel();
        let (said, heard) = oneshot::channel();
        reader.start(async move {
            if gate.await.is_ok() {
                let _ = said.send(());
            }
        });
        (go, heard)
    }

    #[tokio::test]
    async fn a_hush_right_after_a_reader_starts_stops_it() {
        let mut reader = Reader::default();
        let (go, heard) = gated_reader(&mut reader);
        // The hush comes before the task has had a chance to run.
        reader.stop();
        // The gate may already be gone with the aborted task.
        let _ = go.send(());
        assert!(heard.await.is_err(), "a hushed reader still spoke");
    }

    #[tokio::test]
    async fn a_new_reader_stops_the_one_before() {
        let mut reader = Reader::default();
        let (first_go, first) = gated_reader(&mut reader);
        let (second_go, second) = gated_reader(&mut reader);
        let _ = first_go.send(());
        assert!(first.await.is_err(), "the replaced reader still spoke");
        second_go
            .send(())
            .expect("the running reader waits on its gate");
        assert!(second.await.is_ok());
    }
}
