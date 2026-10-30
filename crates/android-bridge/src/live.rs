//! Live voice for the phone apps, as on the Mac: Beep listens, the words
//! show as they are said, a pause or "send" sends, and the reply is read out
//! sentence by sentence before it listens again.
//!
//! The app owns the microphone, the speaker and the chat. It hands the
//! session microphone sound with `push_audio` and the events of the run it
//! started for a `LiveEvent::Send` with `reply_event`, plays each
//! `LiveEvent::Play` and says when it finished with `played`. `beep_live::Live`
//! decides; one task per session runs its effects: live transcription, the
//! `Say` and `Filler` calls, and what the app is told.

use crate::BeepError;
use beep_live::{Effect, EndMode, Filler, Fillers, Input, Leaving, Live, Phase, Reading};
use beep_proto::RunEvent;
use std::{
    collections::VecDeque,
    future::Future,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};
use tokio::{
    sync::mpsc,
    task::AbortHandle,
    time::{MissedTickBehavior, interval, sleep},
};

/// How often pauses are checked.
const TICK: Duration = Duration::from_millis(50);
/// Microphone stretches waiting to go up, a few seconds' worth: enough to
/// cover opening the transcription.
const MIC_CHUNKS: usize = 64;
/// Calls from the app waiting for the session.
const COMMANDS: usize = 64;
/// News from the session's own tasks waiting for it.
const NEWS: usize = 64;
/// Sentences waiting for their audio.
const LINES: usize = 64;
/// Loudness above which the microphone counts as hearing someone.
const SOUND_LEVEL: f32 = 0.12;
/// Failed transcriptions in a row before live mode gives up.
const STT_TRIES: u32 = 3;
/// Wait before opening a transcription again after one failed.
const STT_RETRY: Duration = Duration::from_secs(1);
const UNAVAILABLE: &str = "Voice is not available right now";

/// Where live mode is, for the app's live screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LivePhase {
    Listening,
    /// The user holds the talk control: listening, and letting go sends.
    Holding,
    /// The message went; waiting for the answer.
    Thinking,
    Speaking,
}

impl From<Phase> for LivePhase {
    fn from(phase: Phase) -> Self {
        match phase {
            Phase::Listening => Self::Listening,
            Phase::Holding => Self::Holding,
            Phase::Thinking => Self::Thinking,
            Phase::Speaking => Self::Speaking,
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
    /// The tool step the reply is on.
    pub step: Option<String>,
    /// Whether Beep's voice is playing.
    pub speaking: bool,
}

/// What a live session tells the app.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum LiveEvent {
    View {
        view: LiveView,
    },
    /// A spoken command was heard, such as "Cancelled".
    Toast {
        text: String,
    },
    /// A pause sends after `after_ms`, or no longer will when it is null.
    Soon {
        after_ms: Option<u64>,
    },
    /// Send `text` as a spoken message (`BeepClient::chat_spoken`), in a new
    /// chat when `new_chat`, and hand its run's events to `reply_event`.
    Send {
        text: String,
        new_chat: bool,
    },
    /// Play `audio`, an MP3, then call `played(id)`. `filler` marks a short
    /// line said while a tool runs.
    Play {
        id: u64,
        text: String,
        audio: Vec<u8>,
        filler: bool,
    },
    /// Stop what plays now; no `played` is needed for it.
    StopAudio,
    /// Live mode is over. `draft` is what was said and not sent, `quiet`
    /// says it ended after two minutes with nothing said, and `failure` is
    /// why it could not go on, for the screen.
    Left {
        draft: Option<String>,
        quiet: bool,
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
    Words(String),
    /// The server has no live transcription.
    Unavailable,
    Failed(String),
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
    /// telling `heard` the words.
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
}

type Mic = Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>;

impl LiveSession {
    /// Starts a session on the tokio runtime this is called from.
    pub(crate) fn start<S: Speech>(
        speech: Arc<S>,
        listener: Arc<dyn LiveListener>,
        start: LiveStart,
    ) -> Arc<Self> {
        let (commands, received) = mpsc::channel(COMMANDS);
        let mic = Mic::default();
        let driver = Driver::new(speech, listener, mic.clone(), start);
        tokio::spawn(driver.run(received));
        Arc::new(Self { commands, mic })
    }

    fn send(&self, command: Command) {
        if self.commands.try_send(command).is_err() {
            // Full only if the session stalled; it ended if closed.
            log::debug!("live session did not take a command");
        }
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
        if level >= SOUND_LEVEL {
            self.send(Command::Sound);
        }
        level
    }

    /// A tap on the orb: stops Beep speaking, skips the answer while it
    /// thinks, and leaves while it listens.
    pub fn tap(&self) {
        self.send(Command::Input(Input::Tap));
    }

    /// Stops Beep speaking, or skips the answer while it thinks, and listens.
    /// Does nothing while it listens.
    pub fn interrupt(&self) {
        self.send(Command::Interrupt);
    }

    /// The talk control went down: listen, and send when it comes up.
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

    /// Leaves live mode, keeping what was said as a draft.
    pub fn stop(&self) {
        self.send(Command::Input(Input::Escape));
    }

    /// Leaves live mode because it cannot work, such as without the
    /// microphone. `reason` goes on the screen.
    pub fn fail(&self, reason: String) {
        self.send(Command::Fail(reason));
    }

    pub fn set_send_on_pause(&self, on: bool) {
        self.send(Command::Mode(end_mode(on)));
    }

    /// An event of the run started for the last `LiveEvent::Send`.
    pub fn reply_event(&self, event: RunEvent) {
        let news = match event {
            RunEvent::TextDelta { delta } => ReplyNews::Text(delta),
            RunEvent::ToolStarted {
                name, description, ..
            } => ReplyNews::Tool(if description.is_empty() {
                name
            } else {
                description
            }),
            RunEvent::RunFinished { status, .. } => ReplyNews::Finished {
                ok: matches!(status.as_str(), "completed" | "waiting"),
            },
            RunEvent::Error { .. } => ReplyNews::Finished { ok: false },
            _ => return,
        };
        self.send(Command::Reply(news));
    }

    /// The spoken message could not be sent.
    pub fn reply_failed(&self) {
        self.send(Command::Reply(ReplyNews::Finished { ok: false }));
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
    Reply(ReplyNews),
    Played(u64),
    Fail(String),
}

/// What the app hears of the run it sent.
#[derive(Debug)]
enum ReplyNews {
    Text(String),
    Tool(String),
    Finished { ok: bool },
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

/// A line with its audio, ready to play.
pub(crate) struct Ready {
    text: String,
    audio: Vec<u8>,
    filler: bool,
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
    /// The run ended: whether it went well.
    finished: Option<bool>,
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
    live: Live,
    news: mpsc::Sender<News>,
    heard: Option<mpsc::Receiver<News>>,
    /// Counts transcriptions, so words from an ended one are dropped.
    stt_run: u64,
    stt_failures: u32,
    transcriptions: Vec<AbortHandle>,
    reader: Option<Reader>,
    readers: u64,
    playing: Option<Playing>,
    plays: u64,
    /// The last reply's sentences, for "repeat that".
    last_reply: Vec<String>,
    line: Option<String>,
    prev: Option<String>,
    step: Option<String>,
    failure: Option<String>,
    over: bool,
}

impl<S: Speech> Driver<S> {
    fn new(speech: Arc<S>, listener: Arc<dyn LiveListener>, mic: Mic, start: LiveStart) -> Self {
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
            live,
            news,
            heard: Some(heard),
            stt_run: 0,
            stt_failures: 0,
            transcriptions: Vec::new(),
            reader: None,
            readers: 0,
            playing: None,
            plays: 0,
            last_reply: Vec::new(),
            line: None,
            prev: None,
            step: None,
            failure: None,
            over: false,
        }
    }

    async fn run(mut self, mut commands: mpsc::Receiver<Command>) {
        let Some(mut news) = self.heard.take() else {
            return;
        };
        // Both starts open with one transcription.
        self.apply(vec![Effect::Transcribe]);
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
        self.emit(LiveEvent::View {
            view: LiveView {
                phase: self.live.phase().into(),
                words: self.live.text(),
                line: self.line.clone(),
                prev: self.prev.clone(),
                step: self.step.clone(),
                speaking: self.playing.is_some(),
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
            Command::Sound => self.input(Input::Sound),
            Command::Mode(mode) => self.live.set_mode(mode),
            Command::Reply(news) => self.reply(news),
            Command::Played(id) => self.played(id),
            Command::Fail(reason) => self.fail(reason),
        }
    }

    /// Hands `input` to the state machine and runs what it asks for.
    fn input(&mut self, input: Input) {
        let before = self.live.phase();
        let effects = self.live.input(input, Instant::now());
        let after = self.live.phase();
        // Back to listening from a reply: nothing more is read out.
        if matches!(after, Phase::Listening | Phase::Holding)
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
            Hearing::Words(text) => {
                self.stt_failures = 0;
                self.input(Input::Words(text));
            }
            Hearing::Unavailable => {
                self.fail(format!(
                    "{UNAVAILABLE}: the server has no live transcription."
                ));
            }
            Hearing::Failed(why) => {
                log::warn!("live transcription: {why}");
                self.stt_failures += 1;
                if self.stt_failures >= STT_TRIES {
                    return self.fail(format!("{UNAVAILABLE}: {why}."));
                }
                let news = self.news.clone();
                tokio::spawn(async move {
                    sleep(STT_RETRY).await;
                    let _ = news.send(News::Retry(run)).await;
                });
            }
        }
    }

    /// Leaves live mode, telling the user `failure`.
    fn fail(&mut self, failure: String) {
        self.failure = Some(failure);
        self.input(Input::Escape);
    }

    fn leave(&mut self, draft: Option<String>, why: Leaving) {
        self.over = true;
        self.hush();
        let failure = self.failure.take();
        self.emit(LiveEvent::Left {
            draft,
            quiet: why == Leaving::Quiet,
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
            finished: (!reply).then_some(true),
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
        self.start_reader(text.clone(), Vec::new());
        self.emit(LiveEvent::Send { text, new_chat });
    }

    fn repeat(&mut self) {
        let lines = std::mem::take(&mut self.last_reply);
        if lines.is_empty() {
            // Nothing was said yet: straight back to listening.
            return self.input(Input::SpeakDone);
        }
        self.start_reader(String::new(), lines);
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

    fn reply(&mut self, news: ReplyNews) {
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
            ReplyNews::Finished { ok } => {
                if reader.finished.is_some() {
                    return;
                }
                reader.finished = Some(ok);
                for line in reader.reading.finish() {
                    self.queue_line(Line::Answer(line));
                }
                self.check_done();
            }
        }
    }

    fn voiced(&mut self, id: u64, ready: Ready) {
        let Some(reader) = self.reader.as_mut().filter(|r| r.id == id) else {
            return;
        };
        reader.pending = reader.pending.saturating_sub(1);
        let answer = !ready.filler;
        if !ready.audio.is_empty() {
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

    /// Back to listening once the run ended and every line was spoken.
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
        if reader.finished == Some(false) {
            self.emit(LiveEvent::Toast {
                text: "Could not send".into(),
            });
            self.input(Input::ReplyFailed);
        } else {
            self.input(Input::SpeakDone);
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
