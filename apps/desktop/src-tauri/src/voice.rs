//! Live voice. A tap or a held Option in a bar opened with two taps starts
//! live mode: the bar shows an orb, the words as they are said, and Beep's
//! answer read aloud sentence by sentence, then it listens again. Started by
//! a hold, it is push to talk: between holds nothing listens. A voice note
//! is read out the same way but never listens.
//!
//! `beep_live::Live` decides; this runs its effects. The microphone and the
//! player share one engine with echo cancellation (`audio`), the words
//! come from the server's live transcription, and each sentence of the
//! reply is spoken through `Say` as it streams in.

use crate::{
    audio::{self, Audio, MicChunk, MicDenied, Played},
    bar,
    beep::{Asking, Beep, Decision, Pick, RunNews, Sent},
    error::DesktopError,
    hotkey::{self, Act, HOLD, HoldStep, Input as Key, TapStep, Taps, WINDOW},
    news::{self, Heard, Teller},
    orb,
    presence::Presence,
    state::NoticeVoice,
};
use anyhow::Context;
use beep_live::{
    Effect, EndMode, Filler, Fillers, Input, Leaving, Live, Phase, Reading, Toast as LiveToast,
};
use beep_proto::{
    ApiError, CancelRun, Filler as FillerLine, Notice, Say, SpeakNotice, Transcription, VoiceNote,
};
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
    sync::{mpsc, oneshot, watch},
    task::AbortHandle,
    time::{interval, sleep},
};
use uuid::Uuid;

mod card;

pub use card::NoticeCard;

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
/// Tries while the server has no transcription free, waiting longer each
/// time, about half a minute in all.
const BUSY_TRIES: u32 = 6;
/// The longest wait between those tries.
const BUSY_WAIT: Duration = Duration::from_secs(8);
/// How long letting go of Option waits for the last words.
const FINISH_WAIT: Duration = Duration::from_secs(2);
/// How long the microphone opened when Option went down stays open for a
/// hold or a tap that never came.
const WARM_FOR: Duration = Duration::from_millis(1500);
const UNAVAILABLE: &str = "Voice is not available right now. Try again in a moment.";
const BUSY: &str = "Voice is busy in another window or on another device. Try again in a moment.";
const MIC_DENIED: &str =
    "Beep cannot hear you. Allow it in System Settings, Privacy & Security, Microphone.";
const NOT_SENT: &str = "Could not send";
const NO_ANSWER: &str = "Beep could not answer";
const NOT_ALOUD: &str = "Could not read the reply aloud";
const NOTHING_TO_REPEAT: &str = "Nothing to repeat";
const QUIET: &str = "Live mode ended after 2 minutes quiet.";

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
        /// Push to talk: nothing listens between holds.
        hold_only: bool,
        /// Option came up and the last words are on their way.
        finishing: bool,
        /// The sheet of what to say is open.
        sheet: bool,
    },
    Level {
        level: f32,
    },
    Toast {
        text: String,
    },
    /// A pause sends after `ms`, or no longer will.
    Soon {
        ms: Option<u64>,
    },
    /// A spoken message reached `session` and its reply, `run`, started.
    /// The bar follows the run into the chat under the dock.
    Sent {
        session: String,
        text: String,
        run: String,
    },
    Left {
        draft: Option<String>,
        session: Option<String>,
        quiet: bool,
        failure: Option<String>,
    },
    /// The reply waits for the user to allow an action.
    Approval(Waiting),
    /// An approval was answered, in the bar or the orb, resuming `run`.
    Answered {
        session: String,
        query_id: String,
        run: String,
        decision: Decision,
    },
}

/// An approval the user answered, and the run the answer resumed.
pub struct Answered {
    pub session: String,
    pub query_id: String,
    pub run: String,
    pub decision: Decision,
}

/// An action the reply live mode sent waits on, and the chat it is in.
#[derive(Debug, Clone, Serialize)]
pub struct Waiting {
    session: String,
    approval: Asking,
}

/// How live mode starts.
enum Start {
    Tap,
    Hold,
    Note {
        note: VoiceNote,
        title: String,
        notice: Box<Notice>,
    },
}

/// Why the audio engine did not open.
enum OpenFailure {
    Denied(String),
    Other(String),
}

/// An audio engine on its way: the microphone's sound, the engine once it
/// runs, and why it did not.
struct Opening {
    heard: mpsc::Receiver<MicChunk>,
    audio: watch::Receiver<Option<Arc<Audio>>>,
    failed: oneshot::Receiver<OpenFailure>,
}

/// Opens the engine off the async workers. Without `listen` it only plays,
/// which needs no microphone permission. Dropping the `Opening` closes it.
fn open_audio(listen: bool) -> Opening {
    let (mic, heard) = mpsc::channel(MIC_CHUNKS);
    let (opened, audio) = watch::channel(None);
    let (fail, failed) = oneshot::channel();
    tauri::async_runtime::spawn(async move {
        let mic = listen.then_some(mic);
        let open = tauri::async_runtime::spawn_blocking(move || Audio::open(mic)).await;
        let failure = match open {
            Ok(Ok(audio)) => {
                // Nobody waiting means live mode already ended, and the
                // engine closes as it drops here.
                let _ = opened.send(Some(Arc::new(audio)));
                return;
            }
            Ok(Err(e)) if e.is::<MicDenied>() => OpenFailure::Denied(format!("{e:#}")),
            Ok(Err(e)) => OpenFailure::Other(format!("{e:#}")),
            Err(e) => OpenFailure::Other(format!("the audio did not start: {e}")),
        };
        // Nobody waiting means nobody needs to hear it.
        let _ = fail.send(failure);
    });
    Opening {
        heard,
        audio,
        failed,
    }
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
    /// The chat before the last message, gone back to when it was not sent.
    chat_before: Option<Uuid>,
    title: Option<String>,
    /// A voice note read out: no transcription, and it ends when read.
    note: bool,
    /// The notice a voice note reads, shown as a banner when it cannot be.
    notice: Option<Box<Notice>>,
    /// The engine once it opened.
    audio: watch::Receiver<Option<Arc<Audio>>>,
    /// Microphone sound for the transcription that runs.
    stt: Option<mpsc::Sender<Vec<u8>>>,
    /// Counts transcriptions, so words from an ended one are dropped.
    stt_run: u64,
    stt_failures: u32,
    /// Tries while the server had no transcription free.
    stt_busy: u32,
    /// The transcription whose last words a released hold waits for.
    finishing: Option<u64>,
    failure: Option<String>,
    /// Counts messages sent, so a reply that ended late changes nothing.
    reply: u64,
    /// The run answering the last message, once it started.
    run: Option<String>,
    /// That run is over: there is nothing left to cancel.
    run_done: bool,
    /// The reply's text so far, for "repeat that".
    reply_text: String,
    /// An answer given while the reply was still read out, to follow once
    /// what came before the approval is said.
    resume: Option<Heard>,
    /// Reading a reply or a note aloud.
    reader: Reader,
    playing: Option<Playing>,
    mic_level: f32,
    /// What the user sent last, so fillers are in its language.
    said: String,
    /// The sentences said aloud last, for "repeat that" when the reply's
    /// text is not known.
    last_reply: Vec<String>,
    line: Option<String>,
    prev: Option<String>,
    step: Option<String>,
    /// The approval the last reply waits on, for an orb opened after it
    /// came.
    waiting: Option<Waiting>,
    /// The sheet of what to say is open.
    sheet: bool,
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
    /// The microphone opened when Option went down in the open bar, for
    /// the press it went down for, so a hold hears its first word.
    warm: Option<(u64, Opening)>,
    /// The new notice the orb's card offers to read aloud.
    card: Option<card::Pending>,
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
    /// The user was told the reply cannot be read aloud.
    warned: bool,
}

impl Voiced {
    fn new(queued: u32) -> Mutex<Self> {
        Mutex::new(Self {
            fillers: Fillers::new(Instant::now()),
            queued,
            warned: false,
        })
    }
}

/// How a reply's stream ended.
#[derive(Debug, Default)]
struct Outcome {
    started: bool,
    failed: Option<String>,
    status: Option<String>,
    asked: bool,
}

impl Outcome {
    /// Whether the run stopped on an approval.
    fn waits(&self) -> bool {
        match self.status.as_deref() {
            Some(status) => status == "waiting",
            None => self.asked && self.failed.is_none(),
        }
    }

    fn ok(&self) -> bool {
        self.failed.is_none()
            && (self.waits() || matches!(self.status.as_deref(), Some("completed" | "cancelled")))
    }
}

/// How a transcription ended.
enum SttEnd {
    /// It is no longer the one running, or live mode is leaving.
    Stale,
    /// The server said all it heard. `tail` is what the words so far left
    /// out, when the whole text starts with them.
    Done {
        tail: Option<String>,
    },
    /// The stream closed without its text.
    Closed,
    Failed {
        busy: bool,
        message: String,
    },
}

/// Whether the server turned a transcription away for want of a slot.
fn busy(api: Option<&ApiError>) -> bool {
    matches!(api, Some(ApiError::Conflict { .. }))
}

/// The sentences of `text`, as the reader says them.
fn sentences(text: &str) -> Vec<String> {
    let mut reading = Reading::default();
    let mut lines = reading.push(text);
    lines.extend(reading.finish());
    lines
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

    /// Tells the bar and the orb about live mode.
    fn emit(&self, event: LiveEvent) {
        for label in [bar::LABEL, orb::LABEL] {
            if let Err(e) = self.app.emit_to(label, "live", &event) {
                log::debug!("telling the {label} about live mode: {e}");
            }
        }
    }

    fn toast(&self, text: &str) {
        self.emit(LiveEvent::Toast { text: text.into() });
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

    /// What live mode is doing, for the Option keys.
    fn voice(&self) -> hotkey::Voice {
        match self.state().live.as_ref() {
            None => hotkey::Voice::Off,
            Some(s) if s.note => hotkey::Voice::Note,
            Some(_) => hotkey::Voice::Live,
        }
    }

    /// Whether session `id` reads a voice note.
    fn is_note(&self, id: u64) -> bool {
        self.state()
            .live
            .as_ref()
            .is_some_and(|s| s.id == id && s.note)
    }

    /// Whether the bar is open, which only the user does.
    fn bar_open(&self) -> bool {
        bar::is_open(&self.app)
    }

    /// The bar was hidden during live mode: live mode carries on in the
    /// orb. A voice note was never in the bar and only shows there.
    pub fn minimize(&self) {
        let id = match self.state().live.as_mut() {
            Some(s) => {
                // The sheet belongs to the bar.
                s.sheet = false;
                s.id
            }
            None => return,
        };
        if let Err(e) = orb::show(&self.app) {
            log::warn!("showing the orb: {e}");
        }
        // A new orb has seen nothing yet.
        self.show(id);
        if let Some(waiting) = self.waiting() {
            self.emit(LiveEvent::Approval(waiting));
        }
    }

    /// The approval the reply live mode sent waits on, if any.
    pub fn waiting(&self) -> Option<Waiting> {
        self.state().live.as_ref()?.waiting.clone()
    }

    /// The reply in live session `id` asks before acting: the orb shows a
    /// card for it.
    fn asking(&self, id: u64, approval: Asking) {
        let waiting = {
            let mut state = self.state();
            let Some(s) = state.live.as_mut().filter(|s| s.id == id) else {
                return;
            };
            let Some(chat) = s.chat else {
                return log::warn!("an approval came for a reply without a chat");
            };
            let waiting = Waiting {
                session: chat.to_string(),
                approval,
            };
            s.waiting = Some(waiting.clone());
            waiting
        };
        self.emit(LiveEvent::Approval(waiting));
    }

    /// An approval was answered: the bar and the orb both drop its card,
    /// and the one that did not answer can follow the run it resumed. When
    /// live mode waits on that approval, the rest of the reply is read out
    /// from what is told to the `Teller` returned.
    pub fn answered(self: &Arc<Self>, answered: Answered) -> Option<Teller> {
        let Answered {
            session,
            query_id,
            run,
            decision,
        } = answered;
        let (teller, heard) = news::channel();
        let follow = {
            let mut state = self.state();
            match state.live.as_mut() {
                Some(s)
                    if s.waiting
                        .as_ref()
                        .is_some_and(|w| w.approval.query_id == query_id) =>
                {
                    s.waiting = None;
                    let ours = s.chat.is_some_and(|c| c.to_string() == session);
                    match s.live.phase() {
                        Phase::Waiting if ours => {
                            s.run = Some(run.clone());
                            s.run_done = false;
                            Some((s.id, Some(heard)))
                        }
                        // The approval came while what was before it is
                        // still read out: the rest follows that.
                        Phase::Thinking | Phase::Speaking if ours => {
                            s.resume = Some(heard);
                            Some((s.id, None))
                        }
                        _ => None,
                    }
                }
                _ => None,
            }
        };
        self.emit(LiveEvent::Answered {
            session,
            query_id,
            run,
            decision,
        });
        let (id, heard) = follow?;
        if let Some(heard) = heard {
            self.input(id, Input::Answered);
            self.follow_reply(id, heard, false);
        }
        Some(teller)
    }

    /// What `notice` does: offered on the orb's card or spoken when the
    /// Voice notes choice says so, the bar is closed, live mode is off, and
    /// the user is here and not on a call. Otherwise it is only a banner.
    pub fn wants(&self, notice: &Notice) -> NoticeVoice {
        let choice = self.beep.notice_voice();
        let fits = choice != NoticeVoice::Off
            && notice.approval.is_none()
            && notice.app.is_none()
            && !(notice.title.trim().is_empty() && notice.body.trim().is_empty())
            && !bar::is_open(&self.app)
            && !self.is_live()
            && Presence::now().can_speak();
        if fits { choice } else { NoticeVoice::Off }
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
        let notice = Box::new(notice);
        self.start(
            Start::Note {
                note,
                title,
                notice,
            },
            chat,
        );
    }

    /// A tap from the bar, such as a click on the orb.
    pub fn tap(self: &Arc<Self>) {
        if self.voice() == hotkey::Voice::Note {
            return self.leave_live();
        }
        self.input_now(Input::Tap);
    }

    /// Leaves live mode, keeping what was said as a draft.
    pub fn leave_live(self: &Arc<Self>) {
        self.input_now(Input::Escape);
    }

    /// Esc or Cancel. Closes the sheet of what to say when it is open.
    /// While a reply is being written or read out, stops it on the server
    /// and listens again with the message that asked for it back in the
    /// words; once it was all written, only stops reading it. Otherwise
    /// leaves, keeping what was said.
    pub fn cancel(self: &Arc<Self>) {
        let input = {
            let mut state = self.state();
            let Some(s) = state.live.as_mut() else {
                return;
            };
            if s.sheet {
                s.sheet = false;
                let id = s.id;
                drop(state);
                return self.show(id);
            }
            let replying = matches!(s.live.phase(), Phase::Thinking | Phase::Speaking);
            if s.note {
                Input::Escape
            } else if replying && s.run_done {
                Input::Tap
            } else {
                Input::Cancel
            }
        };
        self.input_now(input);
    }

    /// Opens or closes the sheet of what to say; `None` toggles it.
    pub fn sheet(&self, open: Option<bool>) {
        let id = {
            let mut state = self.state();
            let Some(s) = state.live.as_mut() else {
                return;
            };
            s.sheet = open.unwrap_or(!s.sheet);
            s.id
        };
        self.show(id);
    }

    /// Cancels run `run` on the server, or the run going on in `chat` when
    /// it had not started yet as far as live mode heard.
    fn cancel_run(&self, chat: Option<Uuid>, run: Option<String>) {
        let beep = self.beep.clone();
        tauri::async_runtime::spawn(async move {
            let run = match (run, chat) {
                (Some(run), _) => run,
                (None, Some(chat)) => match beep.active_run(&chat.to_string()).await {
                    Ok(Some(run)) => run,
                    Ok(None) => return,
                    Err(e) => return log::warn!("finding the reply to cancel: {e}"),
                },
                (None, None) => return,
            };
            if let Err(e) = beep.call(CancelRun { id: run }).await {
                log::warn!("cancelling a spoken reply: {e}");
            }
        });
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
        if !bar::is_open(&self.app) {
            return None;
        }
        self.beep.session().and_then(|s| Uuid::parse_str(&s).ok())
    }

    /// Opens the microphone as Option goes down in the open bar, so a hold
    /// or a tap that follows hears from the start. Only once the user has
    /// allowed the microphone: asking belongs to starting live mode.
    fn warm_up(self: &Arc<Self>, press: u64) {
        if !self.bar_open() || !audio::mic_allowed() {
            return;
        }
        {
            let mut state = self.state();
            if state.live.is_some() || state.warm.is_some() {
                return;
            }
            state.warm = Some((press, open_audio(true)));
        }
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            sleep(WARM_FOR).await;
            me.cool(Some(press));
        });
    }

    /// Closes the microphone opened for press `press`, or for any press.
    fn cool(&self, press: Option<u64>) {
        let mut state = self.state();
        if state
            .warm
            .as_ref()
            .is_some_and(|(p, _)| press.is_none_or(|press| *p == press))
        {
            state.warm = None;
        }
    }

    fn start(self: &Arc<Self>, how: Start, chat: Option<Uuid>) {
        let now = Instant::now();
        let mode = self.end_mode();
        let id = self.lives.fetch_add(1, Ordering::Relaxed) + 1;
        let (live, effects, note) = match how {
            Start::Tap => {
                let (live, effects) = Live::listening(mode, now);
                (live, effects, None)
            }
            Start::Hold => {
                let (live, effects) = Live::hold_to_talk(mode, now);
                (live, effects, None)
            }
            Start::Note {
                note,
                title,
                notice,
            } => {
                let (live, effects) = Live::speaking(mode, now);
                (live, effects, Some((note, title, notice)))
            }
        };
        let card = {
            let mut state = self.state();
            if state.live.is_some() {
                return;
            }
            let card = state.card.take();
            let warm = state.warm.take().map(|(_, opening)| opening);
            // A voice note only plays: it never asks for the microphone.
            let opening = match (&note, warm) {
                (Some(_), _) => open_audio(false),
                (None, Some(warm)) => warm,
                (None, None) => open_audio(true),
            };
            let Opening {
                heard,
                audio,
                failed,
            } = opening;
            let me = self.clone();
            let tasks = vec![
                tauri::async_runtime::spawn(self.clone().pump(id, heard))
                    .inner()
                    .abort_handle(),
                tauri::async_runtime::spawn(self.clone().tick(id))
                    .inner()
                    .abort_handle(),
                tauri::async_runtime::spawn(async move {
                    match failed.await {
                        Ok(OpenFailure::Denied(why)) => me.fail(id, MIC_DENIED, why),
                        Ok(OpenFailure::Other(why)) => me.unavailable(id, why),
                        // The engine opened.
                        Err(_) => {}
                    }
                })
                .inner()
                .abort_handle(),
            ];
            state.live = Some(Session {
                id,
                live,
                chat,
                chat_before: chat,
                title: note.as_ref().map(|(_, title, _)| title.clone()),
                note: note.is_some(),
                notice: None,
                audio,
                stt: None,
                stt_run: 0,
                stt_failures: 0,
                stt_busy: 0,
                finishing: None,
                failure: None,
                reply: 0,
                run: None,
                run_done: true,
                reply_text: String::new(),
                resume: None,
                reader: Reader::default(),
                playing: None,
                mic_level: 0.0,
                said: String::new(),
                last_reply: Vec::new(),
                line: None,
                prev: None,
                step: None,
                waiting: None,
                sheet: false,
                tasks,
            });
            card
        };
        // Live mode takes the orb: a notice card still up becomes a banner.
        if let Some(card) = card {
            self.card_gone(card);
        }
        self.watch_escape();
        if let Some((note, _, notice)) = note {
            if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
                s.notice = Some(notice);
            }
            self.read_note(id, note);
        }
        self.apply(id, effects);
        // Only a voice note starts with the bar closed; it shows in the orb.
        if bar::is_open(&self.app) {
            self.show(id);
        } else {
            self.minimize();
        }
    }

    fn watch_escape(self: &Arc<Self>) {
        let me = self.clone();
        let watched = self.app.run_on_main_thread(move || {
            // Only in the bar: Escape in another app, or in a research
            // window, is not meant for live mode.
            hotkey::watch_escape(move || {
                let focused = bar::is_focused(&me.app);
                if focused {
                    me.cancel();
                }
                focused
            });
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
            if matches!(after, Phase::Listening | Phase::Holding | Phase::Waiting)
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
        // "Repeat that" with nothing to repeat goes back to listening.
        let mut nothing_to_repeat = false;
        for effect in effects {
            log::debug!("live {id}: {}", effect.name());
            match effect {
                // A voice note is only read out: nothing listens.
                Effect::Transcribe if self.is_note(id) => {}
                Effect::Transcribe => self.transcribe(id),
                Effect::EndTranscription => self.end_transcription(id),
                Effect::Send { text, new_chat } => self.send(id, text, new_chat),
                // The reader already plays the reply as it comes.
                Effect::Speak => {}
                Effect::Repeat => nothing_to_repeat = !self.repeat(id),
                Effect::StopSpeaking => {
                    if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
                        s.hush();
                    }
                }
                // Interrupting or leaving mid-reply. A voice note has no run,
                // and a run that is over has nothing to stop.
                Effect::CancelReply => {
                    let cancel = self
                        .state()
                        .live
                        .as_mut()
                        .filter(|s| s.id == id && !s.note && !s.run_done)
                        .map(|s| {
                            s.run_done = true;
                            (s.chat, s.run.take())
                        });
                    if let Some((chat, run)) = cancel {
                        self.cancel_run(chat, run);
                    }
                }
                Effect::Leave { draft, why } => self.leave(id, draft, why),
                Effect::Toast(LiveToast::Repeating) if nothing_to_repeat => {}
                Effect::Toast(toast) => self.toast(toast.text()),
                Effect::Soon(after) => self.emit(LiveEvent::Soon {
                    ms: after.map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
                }),
                Effect::Show => {}
            }
        }
        if nothing_to_repeat {
            self.toast(NOTHING_TO_REPEAT);
            self.input(id, Input::SpeakDone);
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
                hold_only: s.live.hold_only(),
                finishing: s.finishing.is_some(),
                sheet: s.sheet,
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
            s.finishing = None;
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
            s.finishing = None;
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
            Err(e) => {
                let api = match &e {
                    DesktopError::Api(api) => Some(api),
                    _ => None,
                };
                let end = SttEnd::Failed {
                    busy: busy(api),
                    message: e.to_string(),
                };
                return self.stt_ended(id, run, end);
            }
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
        let mut heard = String::new();
        let end = loop {
            match replies.next().await {
                Ok(Some(Transcription::Delta { text })) => {
                    if !self.is_run(id, run) {
                        break SttEnd::Stale;
                    }
                    if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
                        s.stt_failures = 0;
                        s.stt_busy = 0;
                    }
                    heard.push_str(&text);
                    self.input(id, Input::Words(text));
                }
                Ok(Some(Transcription::Done { text })) => {
                    let tail = text.strip_prefix(heard.as_str()).map(str::to_owned);
                    break SttEnd::Done { tail };
                }
                Ok(None) => break SttEnd::Closed,
                Ok(Some(Transcription::Unavailable {})) => {
                    self.unavailable(id, "the server has no live transcription".into());
                    break SttEnd::Stale;
                }
                Err(e) => {
                    break SttEnd::Failed {
                        busy: busy(e.downcast_ref::<ApiError>()),
                        message: format!("{e:#}"),
                    };
                }
            }
        };
        up.abort();
        self.stt_ended(id, run, end);
    }

    /// Transcription `run` ended: a released hold takes its last words and
    /// sends; otherwise live mode must not go deaf, so it opens another.
    fn stt_ended(self: &Arc<Self>, id: u64, run: u64, end: SttEnd) {
        let finishing = self
            .state()
            .live
            .as_ref()
            .is_some_and(|s| s.id == id && s.finishing == Some(run));
        if finishing {
            if let SttEnd::Done { tail: Some(tail) } = &end
                && !tail.trim().is_empty()
            {
                self.input(id, Input::Words(tail.clone()));
            }
            if let SttEnd::Failed { message, .. } = &end {
                log::warn!("live transcription, finishing: {message}");
            }
            return self.finish_hold(id, run);
        }
        let listening = self
            .state()
            .live
            .as_ref()
            .is_some_and(|s| s.id == id && s.stt_run == run && s.stt.is_some());
        if !listening {
            return;
        }
        match end {
            SttEnd::Stale => {}
            // The server ended it, such as at its length limit.
            SttEnd::Done { .. } => self.transcribe(id),
            SttEnd::Closed => self.stt_failed(id, run, false, "the transcription ended".into()),
            SttEnd::Failed { busy, message } => self.stt_failed(id, run, busy, message),
        }
    }

    /// Opens the transcription again, unless it keeps failing. While the
    /// server has none free it waits longer each time, saying nothing.
    fn stt_failed(self: &Arc<Self>, id: u64, run: u64, busy: bool, message: String) {
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
            let tries = if busy {
                &mut s.stt_busy
            } else {
                &mut s.stt_failures
            };
            *tries += 1;
            *tries
        };
        let wait = if busy {
            if tries > BUSY_TRIES {
                return self.fail(id, BUSY, message);
            }
            STT_RETRY
                .saturating_mul(1u32 << (tries - 1).min(4))
                .min(BUSY_WAIT)
        } else {
            if tries >= STT_TRIES {
                return self.unavailable(id, message);
            }
            STT_RETRY
        };
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            sleep(wait).await;
            if me.is_run(id, run) {
                me.transcribe(id);
            }
        });
    }

    /// Option came up after a hold. The words said last may still be on
    /// their way, so the audio ends and the hold waits for the server's last
    /// words, up to `FINISH_WAIT`, before it sends.
    fn release(self: &Arc<Self>) {
        let finishing = {
            let mut state = self.state();
            let Some(s) = state.live.as_mut() else {
                return;
            };
            if s.live.phase() != Phase::Holding || s.finishing.is_some() {
                None
            } else if s.stt.take().is_some() {
                // Dropping the sender ends the audio going up.
                s.finishing = Some(s.stt_run);
                Some((s.id, s.stt_run))
            } else {
                None
            }
        };
        let Some((id, run)) = finishing else {
            return self.input_now(Input::HoldEnd);
        };
        self.show(id);
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            sleep(FINISH_WAIT).await;
            me.finish_hold(id, run);
        });
    }

    /// The hold waiting on transcription `run` ends now.
    fn finish_hold(self: &Arc<Self>, id: u64, run: u64) {
        let finishing = self
            .state()
            .live
            .as_mut()
            .filter(|s| s.id == id && s.finishing == Some(run))
            .map(|s| s.finishing = None)
            .is_some();
        if finishing {
            self.input(id, Input::HoldEnd);
        }
    }

    /// The microphone or the transcription cannot work: leave, keeping
    /// what was said. The reason is logged; the screen says it plainly.
    fn unavailable(self: &Arc<Self>, id: u64, why: String) {
        self.fail(id, UNAVAILABLE, why);
    }

    /// Leaves live mode, telling the user `failure`.
    fn fail(self: &Arc<Self>, id: u64, failure: &str, why: String) {
        log::warn!("live mode is unavailable: {why}");
        if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
            s.failure = Some(failure.into());
        }
        self.input(id, Input::End(Leaving::Failed));
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
        // The bar stays on the chat when the user has it up. From the orb,
        // a draft waits in the chat's box for the next time it opens.
        let reopen = bar::is_open(&self.app);
        orb::hide(&self.app);
        let failure = session.failure.take();
        if !reopen {
            self.notify_left(&session, failure.as_deref(), why, draft.is_some());
        }
        self.emit(LiveEvent::Left {
            draft,
            session: chat.map(|c| c.to_string()),
            quiet: why == Leaving::Quiet,
            failure,
        });
        // Dropping the session closes the microphone.
        drop(session);
        if reopen {
            bar::show(&self.app, None);
        }
    }

    /// Live mode in the orb ended on its own: a banner says why, since the
    /// orb is gone. A voice note that could not play shows as its banner.
    fn notify_left(&self, session: &Session, failure: Option<&str>, why: Leaving, kept: bool) {
        if let (Some(notice), Some(_)) = (session.notice.as_deref(), failure) {
            return crate::notify(&self.app, notice);
        }
        let said = match (failure, why) {
            (Some(failure), _) => failure,
            (None, Leaving::Quiet) => QUIET,
            _ => return,
        };
        let body = if kept {
            format!("{said} Your words are kept in the chat.")
        } else {
            said.to_owned()
        };
        crate::notify_text(&self.app, "Live mode", &body);
    }

    /// Sends what was said, and reads the reply out as it streams.
    fn send(self: &Arc<Self>, id: u64, text: String, new_chat: bool) {
        let (chat, reply) = {
            let mut state = self.state();
            let Some(s) = state.live.as_mut().filter(|s| s.id == id) else {
                return;
            };
            let chat = if new_chat {
                Uuid::new_v4()
            } else {
                s.chat.unwrap_or_else(Uuid::new_v4)
            };
            s.chat_before = s.chat;
            s.chat = Some(chat);
            s.title = None;
            s.reply += 1;
            s.run = None;
            s.run_done = false;
            s.reply_text.clear();
            s.resume = None;
            text.clone_into(&mut s.said);
            s.last_reply.clear();
            s.prev = None;
            s.line = None;
            s.step = None;
            // A new message leaves the question unanswered on the server;
            // the `sent` event takes the orb's card away.
            s.waiting = None;
            (chat, s.reply)
        };
        let (teller, heard) = news::channel();
        let beep = self.beep.clone();
        let me = self.clone();
        // The run goes on in the chat whatever happens to the reader, so
        // it marks itself over here.
        tauri::async_runtime::spawn(async move {
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
                .post_heard(chat, sent, Pick::default(), quiet, Some(&teller))
                .await;
            if let Err(e) = &answer {
                log::warn!("sending a spoken message: {e}");
                teller.tell(RunNews::Failed(e.to_string()));
            }
            me.run_over(id, reply);
            // Dropping the teller ends the reader's stream.
        });
        self.follow_reply(id, heard, true);
    }

    /// The run answering message `reply` of session `id` is over.
    fn run_over(&self, id: u64, reply: u64) {
        if let Some(s) = self
            .state()
            .live
            .as_mut()
            .filter(|s| s.id == id && s.reply == reply)
        {
            s.run = None;
            s.run_done = true;
        }
    }

    /// Reads the reply streaming into `heard` aloud, then tells live mode
    /// how it ended. `announce` says it answers a message just sent, which
    /// the chat shows once its run started.
    fn follow_reply(self: &Arc<Self>, id: u64, heard: Heard, announce: bool) {
        let me = self.clone();
        let reader = async move {
            let said = me
                .state()
                .live
                .as_ref()
                .filter(|s| s.id == id)
                .map(|s| s.said.clone())
                .unwrap_or_default();
            let voiced = Voiced::new(0);
            let feed = Feed::Reply {
                heard,
                said,
                announce,
            };
            let outcome = me.read(id, feed, &voiced).await;
            me.reply_ended(id, outcome);
        };
        if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
            s.reader.start(reader);
        }
    }

    /// The reply's stream ended: back to listening, waiting on an
    /// approval, or saying it failed.
    fn reply_ended(self: &Arc<Self>, id: u64, outcome: Outcome) {
        log::debug!("live {id}: the reply ended, {outcome:?}");
        let resume = {
            let mut state = self.state();
            let Some(s) = state.live.as_mut().filter(|s| s.id == id) else {
                return;
            };
            if !outcome.waits() {
                s.run = None;
                s.run_done = true;
            }
            if !outcome.started && !outcome.ok() {
                // Nothing reached the server: no chat to stay on.
                s.chat = s.chat_before;
            }
            if outcome.waits() {
                s.resume.take()
            } else {
                None
            }
        };
        let input = if outcome.ok() {
            if let Some(heard) = resume {
                // Answered already: the rest of the reply goes on.
                return self.follow_reply(id, heard, false);
            }
            if outcome.waits() {
                Input::Approval
            } else {
                Input::SpeakDone
            }
        } else if outcome.started {
            if let Some(why) = &outcome.failed {
                log::warn!("a spoken reply failed: {why}");
            }
            self.toast(NO_ANSWER);
            Input::ReplyFailed
        } else {
            self.toast(NOT_SENT);
            Input::NotSent
        };
        self.input(id, input);
    }

    /// Reads the last reply again. False when there is nothing to read.
    fn repeat(self: &Arc<Self>, id: u64) -> bool {
        let mut state = self.state();
        let Some(s) = state.live.as_mut().filter(|s| s.id == id) else {
            return true;
        };
        s.hush();
        let mut lines = sentences(&s.reply_text);
        if lines.is_empty() {
            lines = s.last_reply.clone();
        }
        if lines.is_empty() {
            return false;
        }
        let me = self.clone();
        s.reader.start(async move {
            let voiced = Voiced::new(0);
            me.read(id, Feed::Lines(lines), &voiced).await;
            me.input(id, Input::SpeakDone);
        });
        true
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
            let voiced = Voiced::new(1);
            let (_, ticks) = watch::channel(0);
            me.play(id, rx, ticks, &voiced, false).await;
            // Read: it ends here rather than listening for an answer.
            me.input(id, Input::Escape);
        };
        if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
            s.reader.start(reader);
        }
    }

    /// Speaks `feed` line by line: getting each line's audio while the one
    /// before plays. Returns how the reply's stream ended.
    async fn read(self: &Arc<Self>, id: u64, feed: Feed, voiced: &Mutex<Voiced>) -> Outcome {
        let (lines, todo) = mpsc::channel(LINES);
        let (ready, playable) = mpsc::channel(READY);
        let (answers, ticks) = watch::channel(0u64);
        let said = match &feed {
            Feed::Reply { said, .. } => said.clone(),
            Feed::Lines(_) => String::new(),
        };
        let fed = self.feed(id, feed, lines, voiced);
        let voiced_up = self.synthesize(todo, ready, answers, said, voiced);
        let played = self.play(id, playable, ticks, voiced, true);
        let (outcome, (), _) = tokio::join!(fed, voiced_up, played);
        outcome
    }

    /// Turns the reply into lines as it streams, with fillers in its
    /// quiet stretches.
    async fn feed(
        &self,
        id: u64,
        feed: Feed,
        lines: mpsc::Sender<Line>,
        voiced: &Mutex<Voiced>,
    ) -> Outcome {
        let queue = |line: Line| {
            let mut v = lock(voiced);
            v.queued += 1;
            v.fillers.speaking(true, Instant::now());
            line
        };
        let mut outcome = Outcome::default();
        let (mut heard, said, announce) = match feed {
            Feed::Lines(all) => {
                for line in all {
                    if lines.send(queue(Line::Answer(line))).await.is_err() {
                        break;
                    }
                }
                return outcome;
            }
            Feed::Reply {
                heard,
                said,
                announce,
            } => (heard, said, announce),
        };
        let mut reading = Reading::default();
        let mut check = interval(FILLER_CHECK);
        loop {
            tokio::select! {
                news = heard.recv() => match news {
                    Some(RunNews::Started(run)) => {
                        outcome.started = true;
                        self.started(id, run, announce.then_some(said.as_str()));
                    }
                    Some(RunNews::Text(delta)) => {
                        if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
                            s.reply_text.push_str(&delta);
                        }
                        for line in reading.push(&delta) {
                            if lines.send(queue(Line::Answer(line))).await.is_err() {
                                return outcome;
                            }
                        }
                    }
                    Some(RunNews::Tool(step)) => {
                        lock(voiced).fillers.tool(&step);
                        self.set_step(id, Some(step));
                    }
                    Some(RunNews::Approval(approval)) => {
                        outcome.asked = true;
                        self.asking(id, approval);
                    }
                    Some(RunNews::Failed(why)) => outcome.failed = Some(why),
                    Some(RunNews::Finished(status)) => outcome.status = Some(status),
                    None => break,
                },
                _ = check.tick() => {
                    let due = lock(voiced).fillers.due(Instant::now());
                    if let Some(filler) = due
                        && lines.send(queue(Line::Filler(filler))).await.is_err()
                    {
                        return outcome;
                    }
                }
            }
        }
        for line in reading.finish() {
            if lines.send(queue(Line::Answer(line))).await.is_err() {
                break;
            }
        }
        outcome
    }

    /// The run answering session `id`'s message started. A message just
    /// sent, `said`, now shows in its chat.
    fn started(&self, id: u64, run: String, said: Option<&str>) {
        let chat = {
            let mut state = self.state();
            let Some(s) = state.live.as_mut().filter(|s| s.id == id) else {
                return;
            };
            s.run = Some(run.clone());
            s.chat
        };
        let (Some(chat), Some(said)) = (chat, said) else {
            return;
        };
        self.beep.add_live_chat(chat);
        self.beep.set_session(Some(chat.to_string()));
        self.emit(LiveEvent::Sent {
            session: chat.to_string(),
            text: said.to_owned(),
            run,
        });
    }

    /// Tells the user once per reply that it cannot be read aloud.
    fn warn_not_aloud(&self, voiced: &Mutex<Voiced>) {
        let first = !std::mem::replace(&mut lock(voiced).warned, true);
        if first {
            self.toast(NOT_ALOUD);
        }
    }

    /// Gets each line's audio from the server, in order.
    async fn synthesize(
        &self,
        mut todo: mpsc::Receiver<Line>,
        ready: mpsc::Sender<Ready>,
        answers: watch::Sender<u64>,
        said: String,
        voiced: &Mutex<Voiced>,
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
                    if !filler {
                        self.warn_not_aloud(voiced);
                    }
                    // Still counted, so the queue empties; an answer line is
                    // shown without its voice.
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
    /// live mode to speaking. An answer line that cannot play shows as a
    /// caption for about as long as it takes to read.
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
            let shown = !line.filler && !line.text.trim().is_empty();
            if (!line.audio.is_empty() || shown) && !cut {
                if !line.filler && reply && !spoke {
                    self.input_reply_ready(id);
                }
                let played = if line.audio.is_empty() {
                    Err(anyhow::anyhow!("no audio for the line"))
                } else {
                    self.play_line(id, &audio, &line, &mut answers).await
                };
                if let Err(e) = played {
                    if shown {
                        if !line.audio.is_empty() {
                            log::warn!("playing a line: {e:#}");
                            self.warn_not_aloud(voiced);
                        }
                        self.caption(id, &line.text).await;
                    } else {
                        log::warn!("playing a filler: {e:#}");
                    }
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

    /// Shows `text` as Beep's line without its voice, for as long as it
    /// takes to read.
    async fn caption(&self, id: u64, text: &str) {
        if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
            s.prev = s.line.take();
            s.line = Some(text.to_owned());
        }
        self.show(id);
        let words = u64::try_from(text.split_whitespace().count()).unwrap_or(u64::MAX);
        let read = Duration::from_millis(words.saturating_mul(250))
            .clamp(Duration::from_millis(1200), Duration::from_secs(6));
        sleep(read).await;
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
        if taps == TapStep::Two {
            self.on_two_taps();
        }
        match hold {
            HoldStep::Wait(press) => {
                if self.voice() == hotkey::Voice::Off {
                    self.warm_up(press);
                }
                let me = self.clone();
                tauri::async_runtime::spawn(async move {
                    sleep(HOLD).await;
                    me.on_hold_timer(press);
                });
            }
            HoldStep::Send => self.release(),
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

    /// Two taps: open or hide the bar. Over the orb they bring the bar back
    /// with live mode still running; with the bar up they leave live mode,
    /// and the bar opens on the chat. With a notice card up they open its
    /// chat.
    fn on_two_taps(self: &Arc<Self>) {
        self.cool(None);
        match hotkey::on_two(self.voice(), self.bar_open()) {
            // A notice card up: two taps open its chat, as Open does.
            hotkey::Two::Toggle if !self.bar_open() && self.open_any_card() => {}
            hotkey::Two::Toggle => bar::toggle(&self.app),
            hotkey::Two::ShowBar => bar::show(&self.app, None),
            hotkey::Two::Leave => {
                self.leave_live();
                if !self.is_live() {
                    bar::show(&self.app, None);
                }
            }
        }
    }

    /// `HOLD` after Option went down: talk while it stays down.
    fn on_hold_timer(self: &Arc<Self>, press: u64) {
        let act = hotkey::on_held(self.voice(), self.bar_open());
        let (step, tapped) = {
            let mut state = self.state();
            let step = state
                .hold
                .on_timer(press, hotkey::since_press(), Instant::now());
            (step, state.hold.tapped())
        };
        if step != HoldStep::Start {
            // Option went with another key: nothing listens. A tap keeps
            // the microphone for the live mode it may start.
            if !tapped {
                self.cool(Some(press));
            }
            return;
        }
        match act {
            Act::Live => self.input_now(Input::HoldStart),
            Act::Start => {
                let chat = self.open_chat();
                self.start(Start::Hold, chat);
            }
            Act::EndNote | Act::Nothing => self.cool(Some(press)),
        }
    }

    /// A single tap stood alone: interrupt or leave live mode, stop a voice
    /// note, or start live mode in a bar the user opened.
    fn on_tap_timer(self: &Arc<Self>, press: u64) {
        let step = self
            .state()
            .hold
            .on_tap_timer(press, hotkey::since_press(), Instant::now());
        if step != HoldStep::Single {
            return self.cool(Some(press));
        }
        match hotkey::on_single(self.voice(), self.bar_open()) {
            Act::Live => self.input_now(Input::Tap),
            Act::EndNote => self.leave_live(),
            Act::Start => {
                let chat = self.open_chat();
                self.start(Start::Tap, chat);
            }
            Act::Nothing => self.cool(Some(press)),
        }
    }
}

/// What the reader speaks: a reply as it streams, or lines known already.
enum Feed {
    Reply {
        heard: Heard,
        said: String,
        /// The reply answers a message just sent.
        announce: bool,
    },
    Lines(Vec<String>),
}

fn lock(voiced: &Mutex<Voiced>) -> MutexGuard<'_, Voiced> {
    voiced.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::beep::Field;
    use serde_json::json;

    #[test]
    fn the_orb_reads_an_approval_and_its_answer() {
        let waiting = Waiting {
            session: "chat".into(),
            approval: Asking {
                query_id: "q1".into(),
                app: "Fastmail".into(),
                action: "Send email".into(),
                fields: vec![Field {
                    label: "To".into(),
                    value: "Alex".into(),
                }],
            },
        };
        let asked = serde_json::to_value(LiveEvent::Approval(waiting)).expect("it serializes");
        assert_eq!(
            asked,
            json!({
                "kind": "approval",
                "session": "chat",
                "approval": {
                    "query_id": "q1",
                    "app": "Fastmail",
                    "action": "Send email",
                    "fields": [{ "label": "To", "value": "Alex" }],
                },
            })
        );
        let answered = LiveEvent::Answered {
            session: "chat".into(),
            query_id: "q1".into(),
            run: "r1".into(),
            decision: Decision::Always,
        };
        assert_eq!(
            serde_json::to_value(answered).expect("it serializes"),
            json!({ "kind": "answered", "session": "chat", "query_id": "q1", "run": "r1", "decision": "always" })
        );
    }

    #[test]
    fn a_sent_message_names_its_run_for_the_bar() {
        let sent = LiveEvent::Sent {
            session: "chat".into(),
            text: "hi".into(),
            run: "r1".into(),
        };
        assert_eq!(
            serde_json::to_value(sent).expect("it serializes"),
            json!({ "kind": "sent", "session": "chat", "text": "hi", "run": "r1" })
        );
    }

    #[test]
    fn a_reply_that_failed_or_never_started_is_not_ok() {
        let done = |status: &str| Outcome {
            started: true,
            status: Some(status.into()),
            ..Outcome::default()
        };
        assert!(done("completed").ok());
        assert!(done("cancelled").ok());
        assert!(done("waiting").ok() && done("waiting").waits());
        assert!(!done("timed_out").ok());
        let errored = Outcome {
            failed: Some("internal error".into()),
            ..done("completed")
        };
        assert!(!errored.ok());
        // A stream that ended with neither a status nor an error.
        assert!(!Outcome::default().ok());
    }

    #[test]
    fn a_busy_server_is_told_apart() {
        assert!(busy(Some(&ApiError::Conflict {
            message: "live transcriptions busy".into()
        })));
        assert!(!busy(Some(&ApiError::Internal {})));
        assert!(!busy(None));
    }

    #[test]
    fn a_reply_is_repeated_sentence_by_sentence() {
        assert_eq!(
            sentences("Two emails need a reply. Alex asks about Thursday."),
            vec!["Two emails need a reply.", "Alex asks about Thursday."]
        );
        assert!(sentences("  ").is_empty());
    }

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
