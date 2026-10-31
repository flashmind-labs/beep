//! Live voice for the phone apps, as on the Mac: Beep listens, the words
//! show as they are said, a pause or "send" sends, and the reply is read out
//! sentence by sentence before it listens again.
//!
//! The app owns the microphone, the speaker and the chat. It hands the
//! session microphone sound with `push_audio`, sends each `LiveEvent::Send`
//! through the chat and hands that run's events back with `reply_event`,
//! plays each `LiveEvent::Play` and says when it finished with `played`.
//! `beep_live::Live` decides; one task per session runs its effects: live
//! transcription, the `Say` and `Filler` calls, and what the app is told.
//!
//! Each `Send` carries a `reply` number. The app passes it back with every
//! call about that run, so events of a reply the user interrupted are
//! dropped instead of being read out as the next one.

use crate::BeepError;
use beep_live::{Effect, EndMode, Filler, Fillers, Input, Leaving, Live, Phase, Reading};
use beep_proto::{Approval, DeviceQuery, RunEvent};
use std::{
    collections::VecDeque,
    future::Future,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    sync::mpsc,
    task::AbortHandle,
    time::{MissedTickBehavior, interval, sleep},
};

/// How often pauses are checked.
const TICK: Duration = Duration::from_millis(50);
/// Microphone stretches waiting to go up: about five seconds of 20 ms
/// chunks, enough to cover opening the transcription, so the app can push
/// sound as soon as the session exists.
const MIC_CHUNKS: usize = 256;
/// Calls from the app waiting for the session. A streamed reply sends one
/// per event, so this is roomy: a dropped one can leave a reply unfinished.
const COMMANDS: usize = 1024;
/// News from the session's own tasks waiting for it.
const NEWS: usize = 64;
/// Sentences waiting for their audio: a long reply's worth.
const LINES: usize = 512;
/// Loudness above which the microphone counts as hearing someone.
const SOUND_LEVEL: f32 = 0.12;
/// Failed transcriptions in a row before live mode gives up.
const STT_TRIES: u32 = 3;
/// How long a transcription must have run for its end to be the server's
/// time limit rather than a failure.
const STT_LIVED: Duration = Duration::from_secs(30);
/// Wait before opening a transcription again after one failed.
const STT_RETRY: Duration = Duration::from_secs(1);
const UNAVAILABLE: &str = "Voice is not available right now";
/// The step shown while the app gets a reply's stream back.
const RECONNECTING: &str = "Reconnecting";

/// Where live mode is, for the app's live screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LivePhase {
    /// The transcription is not open yet, or opens again after it failed:
    /// sound pushed now is kept and heard once it opens. Not "Listening"
    /// yet on screen.
    Connecting,
    Listening,
    /// The user holds the talk control: listening, and letting go sends.
    Holding,
    /// The message went; waiting for the answer. "Stop" and "bye" are
    /// still heard.
    Thinking,
    Speaking,
    /// The reply waits on the approval of the last `LiveEvent::Approval`.
    /// Listening, but quiet never ends live mode.
    Waiting,
}

impl From<Phase> for LivePhase {
    fn from(phase: Phase) -> Self {
        match phase {
            Phase::Listening => Self::Listening,
            Phase::Holding => Self::Holding,
            Phase::Thinking => Self::Thinking,
            Phase::Speaking => Self::Speaking,
            Phase::Waiting => Self::Waiting,
        }
    }
}

/// What the live screen shows.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LiveView {
    pub phase: LivePhase,
    /// What the user said so far this turn, without spoken commands.
    pub words: String,
    /// Beep's sentence playing or played last, and the one before it.
    pub line: Option<String>,
    pub prev: Option<String>,
    /// The tool step the reply is on, or "Reconnecting" while the app gets
    /// the reply's stream back.
    pub step: Option<String>,
    /// Whether Beep's voice is playing.
    pub speaking: bool,
    /// The server could not voice the last line Beep said, so it came as a
    /// `Play` without audio. The screen may say "Beep's voice is
    /// unavailable".
    pub voice_unavailable: bool,
}

/// Why live mode ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LeaveReason {
    /// The user left: `stop`, a tap while listening, "bye", or dropping the
    /// session.
    Asked,
    /// Two minutes with nothing said.
    Quiet,
    /// Another app took the audio, such as a call (`interrupted`).
    Interrupted,
    /// It could not go on; `LiveEvent::Left::failure` says why.
    Failed,
}

impl From<Leaving> for LeaveReason {
    fn from(why: Leaving) -> Self {
        match why {
            Leaving::Asked => Self::Asked,
            Leaving::Quiet => Self::Quiet,
            Leaving::Interrupted => Self::Interrupted,
            Leaving::Failed => Self::Failed,
        }
    }
}

/// How the run for a `Send` failed, as the app saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ReplyFailure {
    /// The message could not be sent, such as `chat_spoken` failing.
    NotSent,
    /// The run's stream broke and could not be resumed.
    ConnectionLost,
}

/// What a live session tells the app.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum LiveEvent {
    View {
        view: LiveView,
    },
    /// A short note to show for a moment: a spoken command was heard, such
    /// as "Cancelled", or the reply failed ("Could not send", "Beep
    /// couldn't finish", "Connection lost").
    Toast {
        text: String,
    },
    /// A pause sends after `after_ms`, or no longer will when it is null.
    Soon {
        after_ms: Option<u64>,
    },
    /// Send `text` as a spoken message (`BeepClient::chat_spoken`), in a new
    /// chat when `new_chat`, and hand its run's events to
    /// `reply_event(reply, ..)`. A card of an earlier `Approval` goes away.
    Send {
        reply: u64,
        text: String,
        new_chat: bool,
    },
    /// The user interrupted reply `reply`, or left in the middle of it:
    /// cancel its run on the server (`BeepClient::cancel_run` with
    /// `run_id`, the `RunStarted` id the session saw, or the chat's
    /// `active_run` when it is null) and stop handing its events. Events
    /// still handed in are dropped.
    CancelReply {
        reply: u64,
        run_id: Option<String>,
    },
    /// Reply `reply` stopped on a tool call that waits for the user's OK,
    /// and everything before it was read out. Show the approval card; the
    /// phase is `Waiting` until it is answered. Answer it with
    /// `BeepClient::answer_approval(chat, query_id, ..)`, call
    /// `approval_answered(reply)`, then hand the events of the run it
    /// returns to `reply_event(reply, ..)` as before. The card goes away on
    /// the next `Send` or `Left`.
    Approval {
        reply: u64,
        /// The run that stopped, when the session saw its `RunStarted`.
        run_id: Option<String>,
        query_id: String,
        approval: Approval,
    },
    /// Play `audio`, an MP3, then call `played(id)`. `filler` marks a short
    /// line said while a tool runs. Empty `audio` means the server could
    /// not voice the line: show `text` for a moment, or say it with the
    /// device's own voice, then call `played(id)`.
    Play {
        id: u64,
        text: String,
        audio: Vec<u8>,
        filler: bool,
    },
    /// Stop what plays now; no `played` is needed for it.
    StopAudio,
    /// Live mode is over. `draft` is what was said and not sent, `reason`
    /// why it ended, and `failure` what to show when it is `Failed`.
    Left {
        draft: Option<String>,
        reason: LeaveReason,
        failure: Option<String>,
    },
}

/// Gets what a live session has to say. Called from a background thread.
#[uniffi::export(with_foreign)]
pub trait LiveListener: Send + Sync {
    fn on_event(&self, event: LiveEvent);
}

/// What a transcription heard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Hearing {
    /// The transcription opened: sound now goes up.
    Open,
    Words(String),
    /// The server has no live transcription.
    Unavailable,
    Failed(String),
    /// The server ended the transcription, such as at its time limit.
    Ended,
}

/// Where one transcription sends what it heard.
#[derive(Clone)]
pub(crate) struct HeardSink {
    run: u64,
    news: mpsc::Sender<News>,
}

impl HeardSink {
    pub(crate) async fn hear(&self, hearing: Hearing) {
        // A session that ended no longer listens.
        let _ = self.news.send(News::Heard(self.run, hearing)).await;
    }
}

/// A line spoken by the server: its words and the MP3.
pub(crate) struct Spoken {
    pub(crate) text: String,
    pub(crate) audio: Vec<u8>,
}

/// The server calls live mode makes, so tests can stand in for it.
pub(crate) trait Speech: Send + Sync + 'static {
    /// Transcribes the 16 kHz PCM16 chunks from `audio` until it closes,
    /// telling `heard` it opened and then the words.
    fn transcribe(
        &self,
        audio: mpsc::Receiver<Vec<u8>>,
        heard: HeardSink,
    ) -> impl Future<Output = ()> + Send;

    /// `text` spoken as it is.
    fn say(&self, text: String) -> impl Future<Output = Result<Vec<u8>, BeepError>> + Send;

    /// A short line for a quiet stretch while a tool runs.
    fn filler(
        &self,
        filler: Filler,
        said: String,
    ) -> impl Future<Output = Result<Spoken, BeepError>> + Send;
}

/// How a live session starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct LiveStart {
    /// A pause after speech sends it; otherwise only "send" does.
    pub send_on_pause: bool,
    /// The user holds the talk control: letting go sends.
    pub holding: bool,
}

/// A running live session. Dropping it leaves live mode.
#[derive(uniffi::Object)]
pub struct LiveSession {
    commands: mpsc::Sender<Command>,
    /// The audio of the transcription that runs, if one does.
    mic: Mic,
    /// A `Sound` waits in `commands`. Loud audio arrives in bursts (the
    /// app flushes what it held while connecting), and one waiting `Sound`
    /// says as much as a hundred, so the rest stay out of the queue and
    /// cannot push out taps or holds.
    sound: Arc<AtomicBool>,
}

type Mic = Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>;

impl LiveSession {
    /// Starts a session on the tokio runtime this is called from. The first
    /// transcription's audio queue exists when this returns, so sound
    /// pushed at once waits for it to open.
    pub(crate) fn start<S: Speech>(
        speech: Arc<S>,
        listener: Arc<dyn LiveListener>,
        start: LiveStart,
    ) -> Arc<Self> {
        let (commands, received) = mpsc::channel(COMMANDS);
        let mic = Mic::default();
        let sound = Arc::new(AtomicBool::new(false));
        let mut driver = Driver::new(speech, listener, mic.clone(), sound.clone(), start);
        driver.transcribe();
        tokio::spawn(driver.run(received));
        Arc::new(Self {
            commands,
            mic,
            sound,
        })
    }

    fn send(&self, command: Command) {
        if self.commands.try_send(command).is_err() {
            // Full only if the session stalled; it ended if closed.
            log::warn!("live session did not take a command");
        }
    }

    fn reply(&self, reply: u64, news: ReplyNews) {
        self.send(Command::Reply(reply, news));
    }
}

impl Drop for LiveSession {
    fn drop(&mut self) {
        // The session also ends once this sender is gone.
        let _ = self.commands.try_send(Command::Input(Input::Escape));
    }
}

#[uniffi::export]
impl LiveSession {
    /// Microphone sound: mono 16-bit little-endian PCM at 16 kHz, in chunks
    /// of about 20 to 100 ms. Returns its level from 0 to 1, for the orb.
    /// Sound pushed while `Connecting` is kept, up to a few seconds.
    pub fn push_audio(&self, pcm: Vec<u8>) -> f32 {
        let level = beep_live::pcm16_level(&pcm);
        let stt = self
            .mic
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(stt) = stt {
            // A full queue drops a stretch rather than falling behind.
            let _ = stt.try_send(pcm);
        }
        if level >= SOUND_LEVEL
            && !self.sound.swap(true, Ordering::AcqRel)
            && self.commands.try_send(Command::Sound).is_err()
        {
            // Not queued, so the next loud stretch tries again.
            self.sound.store(false, Ordering::Release);
        }
        level
    }

    /// A tap on the orb: stops the reply while Beep thinks or speaks
    /// (`CancelReply`), and leaves while it listens.
    pub fn tap(&self) {
        self.send(Command::Input(Input::Tap));
    }

    /// Stops the reply while Beep thinks or speaks (`CancelReply`) and
    /// listens. Does nothing while it listens.
    pub fn interrupt(&self) {
        self.send(Command::Interrupt);
    }

    /// The talk control went down: listen, and send when it comes up. Stops
    /// a reply going on.
    pub fn hold_start(&self) {
        self.send(Command::Input(Input::HoldStart));
    }

    /// The talk control came up: sends what was said while it was held.
    pub fn hold_end(&self) {
        self.send(Command::Input(Input::HoldEnd));
    }

    /// The hold was cancelled, such as by sliding away: drops what was said.
    pub fn hold_cancel(&self) {
        self.send(Command::Input(Input::HoldCancel));
    }

    /// Leaves live mode, keeping what was said as a draft. A reply going on
    /// is stopped (`CancelReply`).
    pub fn stop(&self) {
        self.send(Command::Input(Input::Escape));
    }

    /// Leaves live mode because another app took the audio, such as a
    /// call. Like `stop`, with `LeaveReason::Interrupted`.
    pub fn interrupted(&self) {
        self.send(Command::Input(Input::End(Leaving::Interrupted)));
    }

    /// Leaves live mode because it cannot work, such as without the
    /// microphone. `reason` goes on the screen.
    pub fn fail(&self, reason: String) {
        self.send(Command::Fail(reason));
    }

    pub fn set_send_on_pause(&self, on: bool) {
        self.send(Command::Mode(end_mode(on)));
    }

    /// An event of the run started for `LiveEvent::Send` number `reply`, or
    /// of the run an answered approval resumed.
    pub fn reply_event(&self, reply: u64, event: RunEvent) {
        let news = match event {
            RunEvent::RunStarted { run_id, .. } => ReplyNews::Started(run_id),
            RunEvent::TextDelta { delta } => ReplyNews::Text(delta),
            RunEvent::ToolStarted {
                name, description, ..
            } => ReplyNews::Tool(if description.is_empty() {
                name
            } else {
                description
            }),
            RunEvent::DeviceQuery {
                query_id,
                query: DeviceQuery::Approve { approval },
            } => ReplyNews::Asking(Asking { query_id, approval }),
            RunEvent::RunFinished { status, .. } => ReplyNews::Finished(match status.as_str() {
                "completed" => Outcome::Done,
                // Stopped elsewhere, such as from the chat: nothing to say.
                "cancelled" => Outcome::Done,
                "waiting" => Outcome::Waiting,
                _ => Outcome::Failed(Failure::Unfinished),
            }),
            RunEvent::Error { .. } => ReplyNews::Finished(Outcome::Failed(Failure::Unfinished)),
            _ => return,
        };
        self.reply(reply, news);
    }

    /// The run for `reply` could not start, or its stream broke for good.
    pub fn reply_failed(&self, reply: u64, failure: ReplyFailure) {
        let failure = match failure {
            ReplyFailure::NotSent => Failure::NotSent,
            ReplyFailure::ConnectionLost => Failure::ConnectionLost,
        };
        self.reply(reply, ReplyNews::Finished(Outcome::Failed(failure)));
    }

    /// The stream of `reply` broke and the app is getting it back
    /// (`BeepClient::events`): the screen shows "Reconnecting" until the
    /// next event of it.
    pub fn reconnecting(&self, reply: u64) {
        self.reply(reply, ReplyNews::Reconnecting);
    }

    /// The approval `reply` waited on was answered, here or anywhere else:
    /// Beep thinks again and reads out the events handed to
    /// `reply_event(reply, ..)` next. An event of the resumed run handed in
    /// first does the same.
    pub fn approval_answered(&self, reply: u64) {
        self.send(Command::Answered(reply));
    }

    /// The `LiveEvent::Play` with `id` finished playing, or could not play.
    pub fn played(&self, id: u64) {
        self.send(Command::Played(id));
    }
}

fn end_mode(send_on_pause: bool) -> EndMode {
    if send_on_pause {
        EndMode::Pause
    } else {
        EndMode::Word
    }
}

/// What the app asks of the session.
#[derive(Debug)]
enum Command {
    Input(Input),
    Interrupt,
    Sound,
    Mode(EndMode),
    Reply(u64, ReplyNews),
    Answered(u64),
    Played(u64),
    Fail(String),
}

/// What the app hears of the run it sent.
#[derive(Debug)]
enum ReplyNews {
    Started(String),
    Text(String),
    Tool(String),
    Asking(Asking),
    Reconnecting,
    Finished(Outcome),
}

/// A tool call the run waits on the user's OK for.
#[derive(Debug, Clone)]
struct Asking {
    query_id: String,
    approval: Approval,
}

/// How a reply's run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Done,
    /// On a tool call waiting for the user's OK.
    Waiting,
    Failed(Failure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    /// The message never went.
    NotSent,
    /// The run started and broke off.
    Unfinished,
    /// The stream broke and could not be resumed.
    ConnectionLost,
}

impl Failure {
    fn text(self) -> &'static str {
        match self {
            Self::NotSent => "Could not send",
            Self::Unfinished => "Beep couldn't finish",
            Self::ConnectionLost => "Connection lost",
        }
    }
}

/// What the session's own tasks tell it.
pub(crate) enum News {
    Heard(u64, Hearing),
    /// A line has its audio, for reader `reading`.
    Voiced(u64, Ready),
    /// Time to open transcription `run` again after it failed.
    Retry(u64),
}

/// A line for the synthesizer.
enum Line {
    Answer(String),
    Filler(Filler),
}

/// A line with its audio, ready to play. An answer whose audio failed
/// comes with none, so its words still show.
pub(crate) struct Ready {
    text: String,
    audio: Vec<u8>,
    filler: bool,
}

/// The reply to the last `Send`, while its events are wanted.
struct ReplyRun {
    id: u64,
    run_id: Option<String>,
    /// What the user said, for fillers.
    said: String,
    /// An event of the run came in.
    started: bool,
    /// The run ended; no more events are expected.
    finished: bool,
    asking: Option<Asking>,
    /// Stopped on `asking`, which the app shows.
    waiting: bool,
}

/// Reading a reply, or the last one again, aloud.
struct Reader {
    /// Which reader this is, so audio for an earlier one is dropped.
    id: u64,
    /// A reply streaming in, rather than lines known already.
    reply: bool,
    reading: Reading,
    fillers: Fillers,
    lines: mpsc::Sender<Line>,
    synth: AbortHandle,
    /// Lines sent for audio and not back yet.
    pending: u32,
    queue: VecDeque<Ready>,
    /// The run ended, and how.
    finished: Option<Outcome>,
    spoke: bool,
}

/// The line playing now.
struct Playing {
    id: u64,
    filler: bool,
}

struct Driver<S> {
    speech: Arc<S>,
    listener: Arc<dyn LiveListener>,
    mic: Mic,
    /// Cleared as a `Sound` is taken, so the next loud stretch sends one.
    sound: Arc<AtomicBool>,
    live: Live,
    news: mpsc::Sender<News>,
    heard: Option<mpsc::Receiver<News>>,
    /// Counts transcriptions, so words from an ended one are dropped.
    stt_run: u64,
    stt_failures: u32,
    /// When the current transcription opened.
    stt_opened: Option<Instant>,
    /// A transcription is open; until then the screen shows connecting.
    connected: bool,
    transcriptions: Vec<AbortHandle>,
    reader: Option<Reader>,
    readers: u64,
    /// Counts `Send`s, for their reply numbers.
    replies: u64,
    reply: Option<ReplyRun>,
    playing: Option<Playing>,
    plays: u64,
    /// The last reply's sentences, for "repeat that".
    last_reply: Vec<String>,
    line: Option<String>,
    prev: Option<String>,
    step: Option<String>,
    reconnecting: bool,
    voice_unavailable: bool,
    failure: Option<String>,
    over: bool,
}

impl<S: Speech> Driver<S> {
    fn new(
        speech: Arc<S>,
        listener: Arc<dyn LiveListener>,
        mic: Mic,
        sound: Arc<AtomicBool>,
        start: LiveStart,
    ) -> Self {
        let mode = end_mode(start.send_on_pause);
        let now = Instant::now();
        let (live, _) = if start.holding {
            Live::holding(mode, now)
        } else {
            Live::listening(mode, now)
        };
        let (news, heard) = mpsc::channel(NEWS);
        Self {
            speech,
            listener,
            mic,
            sound,
            live,
            news,
            heard: Some(heard),
            stt_run: 0,
            stt_failures: 0,
            stt_opened: None,
            connected: false,
            transcriptions: Vec::new(),
            reader: None,
            readers: 0,
            replies: 0,
            reply: None,
            playing: None,
            plays: 0,
            last_reply: Vec::new(),
            line: None,
            prev: None,
            step: None,
            reconnecting: false,
            voice_unavailable: false,
            failure: None,
            over: false,
        }
    }

    /// Runs the session. Its first transcription was opened by `start`.
    async fn run(mut self, mut commands: mpsc::Receiver<Command>) {
        let Some(mut news) = self.heard.take() else {
            return;
        };
        self.show();
        let mut tick = interval(TICK);
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        while !self.over {
            tokio::select! {
                command = commands.recv() => match command {
                    Some(command) => self.command(command),
                    None => self.input(Input::Escape),
                },
                Some(news) = news.recv() => self.on_news(news),
                _ = tick.tick() => self.tick(),
            }
        }
        self.close();
    }

    fn emit(&self, event: LiveEvent) {
        self.listener.on_event(event);
    }

    fn show(&self) {
        let phase = match self.live.phase() {
            Phase::Listening | Phase::Holding if !self.connected => LivePhase::Connecting,
            phase => phase.into(),
        };
        let step = if self.reconnecting {
            Some(RECONNECTING.to_owned())
        } else {
            self.step.clone()
        };
        self.emit(LiveEvent::View {
            view: LiveView {
                phase,
                words: self.live.text(),
                line: self.line.clone(),
                prev: self.prev.clone(),
                step,
                speaking: self.playing.is_some(),
                voice_unavailable: self.voice_unavailable,
            },
        });
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::Input(input) => self.input(input),
            Command::Interrupt => {
                if matches!(self.live.phase(), Phase::Thinking | Phase::Speaking) {
                    self.input(Input::Tap);
                }
            }
            Command::Sound => {
                self.sound.store(false, Ordering::Release);
                self.input(Input::Sound);
            }
            Command::Mode(mode) => self.live.set_mode(mode),
            Command::Reply(reply, news) => self.reply(reply, news),
            Command::Answered(reply) => {
                if self
                    .reply
                    .as_ref()
                    .is_some_and(|r| r.id == reply && r.waiting)
                {
                    self.resume();
                }
            }
            Command::Played(id) => self.played(id),
            Command::Fail(reason) => self.fail(reason),
        }
    }

    /// Hands `input` to the state machine and runs what it asks for.
    fn input(&mut self, input: Input) {
        let before = self.live.phase();
        let effects = self.live.input(input, Instant::now());
        let after = self.live.phase();
        // Out of a reply: nothing more is read out.
        if matches!(after, Phase::Listening | Phase::Holding | Phase::Waiting)
            && matches!(before, Phase::Thinking | Phase::Speaking)
        {
            self.hush();
        }
        if !effects.is_empty() {
            self.apply(effects);
            if !self.over {
                self.show();
            }
        }
    }

    fn tick(&mut self) {
        self.input(Input::Tick);
        if self.over {
            return;
        }
        let now = Instant::now();
        let due = self
            .reader
            .as_mut()
            .filter(|r| r.reply && r.finished.is_none())
            .and_then(|r| r.fillers.due(now));
        if let Some(filler) = due {
            self.queue_line(Line::Filler(filler));
        }
    }

    fn apply(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            log::debug!("live: {}", effect.name());
            match effect {
                Effect::Transcribe => self.transcribe(),
                Effect::EndTranscription => self.end_transcription(),
                Effect::Send { text, new_chat } => self.send(text, new_chat),
                // The reader already plays the reply as it comes.
                Effect::Speak | Effect::Show => {}
                Effect::Repeat => self.repeat(),
                Effect::StopSpeaking => self.hush(),
                Effect::CancelReply => self.cancel_reply(),
                Effect::Leave { draft, why } => self.leave(draft, why),
                Effect::Toast(toast) => self.emit(LiveEvent::Toast {
                    text: toast.text().into(),
                }),
                Effect::Soon(after) => self.emit(LiveEvent::Soon {
                    after_ms: after.map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
                }),
            }
        }
    }

    fn set_mic(&self, audio: Option<mpsc::Sender<Vec<u8>>>) {
        *self.mic.lock().unwrap_or_else(PoisonError::into_inner) = audio;
    }

    fn transcribe(&mut self) {
        let (audio, sound) = mpsc::channel(MIC_CHUNKS);
        self.stt_run += 1;
        self.set_mic(Some(audio));
        let heard = HeardSink {
            run: self.stt_run,
            news: self.news.clone(),
        };
        let speech = self.speech.clone();
        self.transcriptions.retain(|t| !t.is_finished());
        let task = tokio::spawn(async move { speech.transcribe(sound, heard).await });
        self.transcriptions.push(task.abort_handle());
    }

    fn end_transcription(&mut self) {
        // Dropping the sender ends the audio; the run's last words no longer
        // count.
        self.set_mic(None);
        self.stt_run += 1;
    }

    fn on_news(&mut self, news: News) {
        match news {
            News::Heard(run, hearing) => self.heard(run, hearing),
            News::Voiced(reader, ready) => self.voiced(reader, ready),
            News::Retry(run) => {
                if run == self.stt_run && !self.over {
                    self.transcribe();
                }
            }
        }
    }

    fn heard(&mut self, run: u64, hearing: Hearing) {
        if run != self.stt_run {
            return;
        }
        match hearing {
            Hearing::Open => {
                self.stt_opened = Some(Instant::now());
                if !self.connected {
                    self.connected = true;
                    self.show();
                }
            }
            Hearing::Words(text) => {
                self.stt_failures = 0;
                self.input(Input::Words(text));
            }
            Hearing::Ended if self.lived() => {
                // The server's time limit: open the next one straight away.
                self.stt_failures = 0;
                if !self.over {
                    self.transcribe();
                }
            }
            Hearing::Ended => self.heard(run, Hearing::Failed("the transcription ended".into())),
            Hearing::Unavailable => {
                self.fail(format!(
                    "{UNAVAILABLE}: the server has no live transcription."
                ));
            }
            Hearing::Failed(why) => {
                log::warn!("live transcription: {why}");
                if self.lived() {
                    // It ran a good while, so earlier failures are behind it.
                    self.stt_failures = 0;
                }
                self.stt_opened = None;
                self.stt_failures += 1;
                if self.stt_failures >= STT_TRIES {
                    return self.fail(format!("{UNAVAILABLE}: {why}."));
                }
                if self.connected {
                    self.connected = false;
                    self.show();
                }
                let news = self.news.clone();
                tokio::spawn(async move {
                    sleep(STT_RETRY).await;
                    let _ = news.send(News::Retry(run)).await;
                });
            }
        }
    }

    /// Whether the current transcription has been open for `STT_LIVED`.
    fn lived(&self) -> bool {
        self.stt_opened
            .is_some_and(|opened| opened.elapsed() >= STT_LIVED)
    }

    /// Leaves live mode, telling the user `failure`.
    fn fail(&mut self, failure: String) {
        self.failure = Some(failure);
        self.input(Input::End(Leaving::Failed));
    }

    fn leave(&mut self, draft: Option<String>, why: Leaving) {
        self.over = true;
        self.hush();
        let failure = self.failure.take();
        self.emit(LiveEvent::Left {
            draft,
            reason: why.into(),
            failure,
        });
    }

    fn close(&mut self) {
        self.hush();
        self.set_mic(None);
        for task in self.transcriptions.drain(..) {
            task.abort();
        }
    }

    /// Stops anything being read out.
    fn hush(&mut self) {
        if let Some(reader) = self.reader.take() {
            reader.synth.abort();
        }
        if self.playing.take().is_some() {
            self.emit(LiveEvent::StopAudio);
        }
        self.step = None;
        self.reconnecting = false;
    }

    /// Drops the reply going on, asking the app to cancel its run when it
    /// has not finished.
    fn cancel_reply(&mut self) {
        let Some(reply) = self.reply.take() else {
            return;
        };
        if !reply.finished && !reply.waiting {
            self.emit(LiveEvent::CancelReply {
                reply: reply.id,
                run_id: reply.run_id,
            });
        }
    }

    /// Starts reading out a new reply, or `lines` known already.
    fn start_reader(&mut self, said: String, lines: Vec<String>) {
        self.hush();
        self.readers += 1;
        let (tx, todo) = mpsc::channel(LINES);
        let synth = tokio::spawn(synthesize(
            self.speech.clone(),
            self.readers,
            todo,
            self.news.clone(),
            said,
        ));
        let reply = lines.is_empty();
        self.reader = Some(Reader {
            id: self.readers,
            reply,
            reading: Reading::default(),
            fillers: Fillers::new(Instant::now()),
            lines: tx,
            synth: synth.abort_handle(),
            pending: 0,
            queue: VecDeque::new(),
            finished: (!reply).then_some(Outcome::Done),
            spoke: false,
        });
        for line in lines {
            self.queue_line(Line::Answer(line));
        }
        self.check_done();
    }

    fn send(&mut self, text: String, new_chat: bool) {
        self.last_reply.clear();
        self.line = None;
        self.prev = None;
        self.replies += 1;
        let reply = self.replies;
        self.reply = Some(ReplyRun {
            id: reply,
            run_id: None,
            said: text.clone(),
            started: false,
            finished: false,
            asking: None,
            waiting: false,
        });
        self.start_reader(text.clone(), Vec::new());
        self.emit(LiveEvent::Send {
            reply,
            text,
            new_chat,
        });
    }

    fn repeat(&mut self) {
        let lines = std::mem::take(&mut self.last_reply);
        if lines.is_empty() {
            // Nothing was said yet: straight back to listening.
            return self.input(Input::SpeakDone);
        }
        self.start_reader(String::new(), lines);
    }

    /// The approval the reply waited on was answered: think again, and read
    /// out the run it resumed.
    fn resume(&mut self) {
        let Some(reply) = self.reply.as_mut() else {
            return;
        };
        reply.waiting = false;
        reply.finished = false;
        reply.asking = None;
        let said = reply.said.clone();
        self.input(Input::Answered);
        if self.live.phase() == Phase::Thinking {
            self.last_reply.clear();
            self.start_reader(said, Vec::new());
            self.show();
        } else {
            // The user is talking over it: the chat shows the rest.
            self.reply = None;
        }
    }

    fn queue_line(&mut self, line: Line) {
        let Some(reader) = self.reader.as_mut() else {
            return;
        };
        if reader.lines.try_send(line).is_err() {
            log::warn!("live: too many lines waiting to be spoken");
            return;
        }
        reader.pending += 1;
        reader.fillers.speaking(true, Instant::now());
    }

    fn reply(&mut self, id: u64, news: ReplyNews) {
        let Some(reply) = self.reply.as_mut().filter(|r| r.id == id) else {
            // An interrupted or older reply.
            return;
        };
        if reply.waiting {
            // The resumed run's first events mean the approval was answered.
            if !matches!(
                news,
                ReplyNews::Started(_) | ReplyNews::Text(_) | ReplyNews::Tool(_)
            ) {
                return;
            }
            self.resume();
            return self.reply(id, news);
        }
        if let ReplyNews::Started(run_id) = &news {
            reply.run_id = Some(run_id.clone());
        }
        if matches!(news, ReplyNews::Reconnecting) {
            if !reply.finished {
                self.reconnecting = true;
                self.show();
            }
            return;
        }
        let was_reconnecting = std::mem::take(&mut self.reconnecting);
        let news = match news {
            ReplyNews::Finished(Outcome::Failed(Failure::NotSent)) if reply.started => {
                ReplyNews::Finished(Outcome::Failed(Failure::Unfinished))
            }
            news => news,
        };
        reply.started = true;
        let news = match news {
            ReplyNews::Asking(asking) => {
                reply.asking = Some(asking);
                if was_reconnecting {
                    self.show();
                }
                return;
            }
            ReplyNews::Finished(outcome) => {
                reply.finished = true;
                ReplyNews::Finished(outcome)
            }
            news => news,
        };
        let Some(reader) = self.reader.as_mut().filter(|r| r.reply) else {
            return;
        };
        match news {
            ReplyNews::Text(delta) => {
                for line in reader.reading.push(&delta) {
                    self.queue_line(Line::Answer(line));
                }
            }
            ReplyNews::Tool(step) => {
                reader.fillers.tool(&step);
                self.step = Some(step);
                self.show();
            }
            ReplyNews::Finished(outcome) => {
                if reader.finished.is_some() {
                    return;
                }
                reader.finished = Some(outcome);
                for line in reader.reading.finish() {
                    self.queue_line(Line::Answer(line));
                }
                self.check_done();
            }
            ReplyNews::Started(_) | ReplyNews::Asking(_) | ReplyNews::Reconnecting => {}
        }
        if was_reconnecting && !self.over {
            self.show();
        }
    }

    fn voiced(&mut self, id: u64, ready: Ready) {
        let Some(reader) = self.reader.as_mut().filter(|r| r.id == id) else {
            return;
        };
        reader.pending = reader.pending.saturating_sub(1);
        let answer = !ready.filler;
        if answer {
            self.voice_unavailable = ready.audio.is_empty();
        }
        // A filler without audio says nothing; an answer without audio
        // still shows its words.
        if !ready.text.is_empty() && (answer || !ready.audio.is_empty()) {
            reader.queue.push_back(ready);
        }
        // An answer ready makes a filler playing now moot.
        if answer && self.playing.as_ref().is_some_and(|p| p.filler) {
            self.playing = None;
            self.emit(LiveEvent::StopAudio);
        }
        self.play_next();
    }

    fn play_next(&mut self) {
        if self.playing.is_some() {
            return;
        }
        let Some(reader) = self.reader.as_mut() else {
            return;
        };
        while let Some(ready) = reader.queue.pop_front() {
            if ready.filler && reader.queue.iter().any(|r| !r.filler) {
                continue;
            }
            let first_answer = !ready.filler && reader.reply && !reader.spoke;
            if !ready.filler {
                reader.spoke = true;
                self.last_reply.push(ready.text.clone());
            }
            self.plays += 1;
            self.playing = Some(Playing {
                id: self.plays,
                filler: ready.filler,
            });
            self.prev = self.line.take();
            self.line = Some(ready.text.clone());
            if first_answer && self.live.phase() == Phase::Thinking {
                self.input(Input::ReplyReady);
            }
            self.emit(LiveEvent::Play {
                id: self.plays,
                text: ready.text,
                audio: ready.audio,
                filler: ready.filler,
            });
            self.show();
            return;
        }
        self.check_done();
    }

    fn played(&mut self, id: u64) {
        if self.playing.as_ref().is_none_or(|p| p.id != id) {
            return;
        }
        self.playing = None;
        if let Some(reader) = self.reader.as_mut()
            && reader.queue.is_empty()
            && reader.pending == 0
        {
            reader.fillers.speaking(false, Instant::now());
        }
        self.show();
        self.play_next();
    }

    /// Once the run ended and every line was spoken: back to listening, or
    /// to waiting on an approval.
    fn check_done(&mut self) {
        let done = self.playing.is_none()
            && self
                .reader
                .as_ref()
                .is_some_and(|r| r.finished.is_some() && r.pending == 0 && r.queue.is_empty());
        if !done {
            return;
        }
        let Some(reader) = self.reader.take() else {
            return;
        };
        reader.synth.abort();
        let outcome = reader.finished.unwrap_or(Outcome::Done);
        if !reader.reply {
            // "Repeat that" finished; a reply waiting on an approval still
            // does.
            return self.input(Input::SpeakDone);
        }
        match outcome {
            Outcome::Done => {
                self.reply = None;
                self.input(Input::SpeakDone);
            }
            Outcome::Waiting => {
                let asked = self.reply.as_mut().and_then(|r| {
                    let asking = r.asking.clone()?;
                    r.waiting = true;
                    Some((r.id, r.run_id.clone(), asking))
                });
                let Some((reply, run_id, asking)) = asked else {
                    // Waiting on nothing this session saw: the chat shows it.
                    self.reply = None;
                    return self.input(Input::SpeakDone);
                };
                self.emit(LiveEvent::Approval {
                    reply,
                    run_id,
                    query_id: asking.query_id,
                    approval: asking.approval,
                });
                self.input(Input::Approval);
            }
            Outcome::Failed(failure) => {
                self.reply = None;
                self.emit(LiveEvent::Toast {
                    text: failure.text().into(),
                });
                self.input(Input::ReplyFailed);
            }
        }
    }
}

/// Gets each line's audio from the server, in order.
async fn synthesize<S: Speech>(
    speech: Arc<S>,
    reader: u64,
    mut todo: mpsc::Receiver<Line>,
    news: mpsc::Sender<News>,
    said: String,
) {
    while let Some(line) = todo.recv().await {
        let ready = match line {
            Line::Answer(text) => match speech.say(text.clone()).await {
                Ok(audio) => Ready {
                    text,
                    audio,
                    filler: false,
                },
                Err(e) => {
                    log::warn!("speaking a line: {e}");
                    // Still counted, so the reader can finish.
                    Ready {
                        text,
                        audio: Vec::new(),
                        filler: false,
                    }
                }
            },
            Line::Filler(filler) => match speech.filler(filler, said.clone()).await {
                Ok(spoken) => Ready {
                    text: spoken.text,
                    audio: spoken.audio,
                    filler: true,
                },
                Err(e) => {
                    log::warn!("speaking a filler: {e}");
                    Ready {
                        text: String::new(),
                        audio: Vec::new(),
                        filler: true,
                    }
                }
            },
        };
        if news.send(News::Voiced(reader, ready)).await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests;
