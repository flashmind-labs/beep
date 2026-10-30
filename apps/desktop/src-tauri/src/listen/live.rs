//! Live mode as a state machine: listening, thinking, speaking and back to
//! listening. It only decides; `voice` opens the microphone, talks to the
//! server and plays the audio for each `Effect`.

use super::{
    command::Command,
    turn::{Closed, Turn},
};
use std::time::{Duration, Instant};

/// Quiet after a phrase before it counts as whole, so a command said on its
/// own is told apart from the start of a sentence.
pub const PHRASE_PAUSE: Duration = Duration::from_millis(1200);
/// Quiet after speech that sends, when pauses send.
pub const PAUSE_SEND: Duration = Duration::from_millis(1500);
/// Quiet before the bar shows that a pause is about to send.
pub const SOON_AFTER: Duration = Duration::from_millis(300);
/// Live mode ends after this long with nothing said.
pub const QUIET_LIMIT: Duration = Duration::from_secs(2 * 60);

/// How a spoken message ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndMode {
    /// A pause after speech sends it; "send" sends at once.
    Pause,
    /// Only "send" sends; pauses never do.
    Word,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Listening,
    /// Option is held: listening, and letting go sends.
    Holding,
    /// The message went; waiting for the answer.
    Thinking,
    Speaking,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Listening => "listening",
            Self::Holding => "holding",
            Self::Thinking => "thinking",
            Self::Speaking => "speaking",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// Option tapped once.
    Tap,
    Escape,
    /// Option held long enough to talk.
    HoldStart,
    /// Option came up after a hold.
    HoldEnd,
    /// Another key went down during a hold.
    HoldCancel,
    /// The microphone heard sound above the noise.
    Sound,
    /// More words from the transcript.
    Words(String),
    /// Time passed; checks pauses.
    Tick,
    /// The answer is ready to be spoken.
    ReplyReady,
    /// The message or its answer failed.
    ReplyFailed,
    /// Beep finished speaking.
    SpeakDone,
}

/// A short note the bar shows when a command was heard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toast {
    Cancelled,
    Scratched,
    NewChat,
    Stopped,
    Repeating,
}

impl Toast {
    pub fn text(self) -> &'static str {
        match self {
            Self::Cancelled => "Cancelled",
            Self::Scratched => "Removed last sentence",
            Self::NewChat => "New chat",
            Self::Stopped => "Stopped",
            Self::Repeating => "Repeating",
        }
    }
}

/// Why live mode ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leaving {
    /// A key or "bye".
    Asked,
    /// Nothing was said for `QUIET_LIMIT`.
    Quiet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Starts a new transcription, ending one that runs.
    Transcribe,
    /// Ends the transcription.
    EndTranscription,
    Send {
        text: String,
        new_chat: bool,
    },
    /// Speaks the answer that is ready.
    Speak,
    /// Speaks the last answer again.
    Repeat,
    StopSpeaking,
    /// Live mode is over; `draft` is what was said and not sent.
    Leave {
        draft: Option<String>,
        why: Leaving,
    },
    Toast(Toast),
    /// A pause will send after this long, or no longer will.
    Soon(Option<Duration>),
    /// The words changed.
    Show,
}

impl Effect {
    /// The effect without what it carries, for the log: no words said.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Transcribe => "transcribe",
            Self::EndTranscription => "end transcription",
            Self::Send { .. } => "send",
            Self::Speak => "speak",
            Self::Repeat => "repeat",
            Self::StopSpeaking => "stop speaking",
            Self::Leave { .. } => "leave",
            Self::Toast(_) => "toast",
            Self::Soon(_) => "soon",
            Self::Show => "show",
        }
    }
}

#[derive(Debug)]
pub struct Live {
    phase: Phase,
    mode: EndMode,
    turn: Turn,
    /// Words heard while Beep speaks, only ever a "stop".
    barge: Turn,
    /// Last sound or word, for pauses.
    active_at: Instant,
    /// Last word, or the start of listening, for `QUIET_LIMIT`.
    heard_at: Instant,
    new_chat: bool,
    /// Whether a pause may send: off after a command until new words.
    armed: bool,
    soon: bool,
    over: bool,
}

impl Live {
    fn new(phase: Phase, mode: EndMode, now: Instant) -> Self {
        Self {
            phase,
            mode,
            turn: Turn::default(),
            barge: Turn::default(),
            active_at: now,
            heard_at: now,
            new_chat: false,
            armed: true,
            soon: false,
            over: false,
        }
    }

    /// Live mode started with a tap: listening.
    pub fn listening(mode: EndMode, now: Instant) -> (Self, Vec<Effect>) {
        (
            Self::new(Phase::Listening, mode, now),
            vec![Effect::Transcribe],
        )
    }

    /// Live mode started by holding Option.
    pub fn holding(mode: EndMode, now: Instant) -> (Self, Vec<Effect>) {
        (
            Self::new(Phase::Holding, mode, now),
            vec![Effect::Transcribe],
        )
    }

    /// Live mode started by a voice note, which the caller plays.
    pub fn speaking(mode: EndMode, now: Instant) -> (Self, Vec<Effect>) {
        (
            Self::new(Phase::Speaking, mode, now),
            vec![Effect::Transcribe],
        )
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn set_mode(&mut self, mode: EndMode) {
        self.mode = mode;
    }

    /// The words of the turn so far, without commands.
    pub fn text(&self) -> String {
        self.turn.text()
    }

    #[cfg(test)]
    pub fn is_over(&self) -> bool {
        self.over
    }

    pub fn input(&mut self, input: Input, now: Instant) -> Vec<Effect> {
        if self.over {
            return Vec::new();
        }
        match (self.phase, input) {
            (_, Input::Escape) => self.leave(Leaving::Asked),
            (Phase::Listening | Phase::Holding, Input::Words(delta)) => {
                self.turn.hear(&delta);
                self.active_at = now;
                self.heard_at = now;
                self.armed = true;
                let mut out = self.not_soon();
                out.push(Effect::Show);
                out
            }
            (Phase::Speaking, Input::Words(delta)) => {
                self.barge.hear(&delta);
                self.active_at = now;
                Vec::new()
            }
            (Phase::Listening | Phase::Holding | Phase::Speaking, Input::Sound) => {
                self.active_at = now;
                self.not_soon()
            }
            (Phase::Listening, Input::Tick) => self.listening_tick(now),
            (Phase::Speaking, Input::Tick) => self.speaking_tick(now),
            (Phase::Listening, Input::Tap) => self.leave(Leaving::Asked),
            (Phase::Listening, Input::HoldStart) => {
                self.phase = Phase::Holding;
                self.not_soon()
            }
            (Phase::Thinking, Input::Tap) => {
                // The answer still arrives in the chat, but is not spoken.
                self.listen(now);
                vec![Effect::Transcribe]
            }
            (Phase::Thinking, Input::HoldStart) => {
                self.listen(now);
                self.phase = Phase::Holding;
                vec![Effect::Transcribe]
            }
            (Phase::Thinking, Input::ReplyReady) => {
                self.phase = Phase::Speaking;
                self.barge.clear();
                vec![Effect::Speak, Effect::Transcribe]
            }
            // The reply had nothing to say aloud.
            (Phase::Thinking, Input::ReplyFailed | Input::SpeakDone) => {
                self.listen(now);
                vec![Effect::Transcribe]
            }
            (Phase::Speaking, Input::Tap) => {
                self.listen(now);
                vec![
                    Effect::StopSpeaking,
                    Effect::Transcribe,
                    Effect::Toast(Toast::Stopped),
                ]
            }
            (Phase::Speaking, Input::HoldStart) => {
                self.listen(now);
                self.phase = Phase::Holding;
                vec![Effect::StopSpeaking, Effect::Transcribe]
            }
            (Phase::Speaking, Input::SpeakDone) => {
                self.listen(now);
                vec![Effect::Transcribe]
            }
            (Phase::Holding, Input::HoldEnd) => {
                self.turn.close();
                if self.turn.is_empty() {
                    self.phase = Phase::Listening;
                    return vec![Effect::Show];
                }
                self.send()
            }
            (Phase::Holding, Input::HoldCancel) => {
                self.turn.clear();
                self.phase = Phase::Listening;
                vec![Effect::Toast(Toast::Cancelled), Effect::Show]
            }
            _ => Vec::new(),
        }
    }

    /// Back to listening for a new turn, with the pause clocks restarted.
    fn listen(&mut self, now: Instant) {
        self.phase = Phase::Listening;
        self.barge.clear();
        self.active_at = now;
        self.heard_at = now;
        self.soon = false;
    }

    fn not_soon(&mut self) -> Vec<Effect> {
        if std::mem::take(&mut self.soon) {
            vec![Effect::Soon(None)]
        } else {
            Vec::new()
        }
    }

    fn listening_tick(&mut self, now: Instant) -> Vec<Effect> {
        let quiet = now.saturating_duration_since(self.active_at);
        if self.turn.is_open()
            && quiet >= PHRASE_PAUSE
            && let Closed::Command(command) = self.turn.close()
        {
            return self.command(command);
        }
        if self.mode == EndMode::Pause && self.armed && !self.turn.is_empty() {
            if quiet >= PAUSE_SEND {
                return self.send();
            }
            if quiet >= SOON_AFTER && !self.soon {
                self.soon = true;
                return vec![Effect::Soon(Some(PAUSE_SEND - quiet))];
            }
        }
        if now.saturating_duration_since(self.heard_at) >= QUIET_LIMIT {
            return self.leave(Leaving::Quiet);
        }
        Vec::new()
    }

    fn speaking_tick(&mut self, now: Instant) -> Vec<Effect> {
        let quiet = now.saturating_duration_since(self.active_at);
        if !self.barge.is_open() || quiet < PHRASE_PAUSE {
            return Vec::new();
        }
        let closed = self.barge.close();
        self.barge.clear();
        match closed {
            Closed::Command(Command::Stop) => {
                self.listen(now);
                vec![
                    Effect::StopSpeaking,
                    Effect::Transcribe,
                    Effect::Toast(Toast::Stopped),
                ]
            }
            Closed::Command(Command::Leave) => self.leave(Leaving::Asked),
            // Anything else may be Beep's own voice coming back.
            _ => Vec::new(),
        }
    }

    fn command(&mut self, command: Command) -> Vec<Effect> {
        let mut out = self.not_soon();
        match command {
            Command::Send => return self.send(),
            Command::Cancel => {
                self.turn.clear();
                self.armed = false;
                out.extend([Effect::Toast(Toast::Cancelled), Effect::Show]);
            }
            Command::ScratchThat => {
                self.turn.scratch();
                self.armed = false;
                out.extend([Effect::Toast(Toast::Scratched), Effect::Show]);
            }
            Command::NewChat => {
                self.turn.clear();
                self.new_chat = true;
                self.armed = false;
                out.extend([Effect::Toast(Toast::NewChat), Effect::Show]);
            }
            Command::Repeat => {
                self.armed = false;
                self.phase = Phase::Speaking;
                self.barge.clear();
                out.extend([
                    Effect::Repeat,
                    Effect::Transcribe,
                    Effect::Toast(Toast::Repeating),
                ]);
            }
            Command::Leave => return self.leave(Leaving::Asked),
            // Nothing is speaking.
            Command::Stop => out.push(Effect::Show),
        }
        out
    }

    fn send(&mut self) -> Vec<Effect> {
        let text = self.turn.text();
        let mut out = self.not_soon();
        if text.is_empty() {
            return out;
        }
        self.turn.clear();
        self.phase = Phase::Thinking;
        out.extend([
            Effect::EndTranscription,
            Effect::Send {
                text,
                new_chat: std::mem::take(&mut self.new_chat),
            },
        ]);
        out
    }

    fn leave(&mut self, why: Leaving) -> Vec<Effect> {
        self.over = true;
        let draft = Some(self.turn.text()).filter(|t| !t.is_empty());
        let mut out = vec![Effect::EndTranscription];
        if self.phase == Phase::Speaking {
            out.push(Effect::StopSpeaking);
        }
        out.push(Effect::Leave { draft, why });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(t: Instant, ms: u64) -> Instant {
        t + Duration::from_millis(ms)
    }

    fn sends(effects: &[Effect]) -> Option<&str> {
        effects.iter().find_map(|e| match e {
            Effect::Send { text, .. } => Some(text.as_str()),
            _ => None,
        })
    }

    fn says(live: &mut Live, words: &str, now: Instant) {
        live.input(Input::Words(words.into()), now);
    }

    #[test]
    fn a_pause_sends_in_pause_mode() {
        let t = Instant::now();
        let (mut live, start) = Live::listening(EndMode::Pause, t);
        assert_eq!(start, vec![Effect::Transcribe]);
        says(&mut live, "What's the weather in Girona", at(t, 100));
        assert_eq!(
            live.input(Input::Tick, at(t, 500)),
            vec![Effect::Soon(Some(Duration::from_millis(1100)))]
        );
        // Talking again takes the countdown away.
        says(&mut live, " this weekend", at(t, 700));
        let out = live.input(Input::Tick, at(t, 700 + 1500));
        assert_eq!(
            sends(&out),
            Some("What's the weather in Girona this weekend")
        );
        assert_eq!(live.phase(), Phase::Thinking);
    }

    #[test]
    fn a_pause_never_sends_in_word_mode() {
        let t = Instant::now();
        let (mut live, _) = Live::listening(EndMode::Word, t);
        says(&mut live, "Remind me to call the plumber", at(t, 100));
        for ms in (200..10_000).step_by(100) {
            assert_eq!(sends(&live.input(Input::Tick, at(t, ms))), None);
        }
        says(&mut live, ", send.", at(t, 10_000));
        let out = live.input(Input::Tick, at(t, 10_000) + PHRASE_PAUSE);
        assert_eq!(sends(&out), Some("Remind me to call the plumber"));
    }

    #[test]
    fn send_sends_at_once_in_pause_mode_too() {
        let t = Instant::now();
        let (mut live, _) = Live::listening(EndMode::Pause, t);
        says(&mut live, "Buy milk. Send.", at(t, 100));
        live.input(Input::Sound, at(t, 1000));
        let out = live.input(Input::Tick, at(t, 1000) + PHRASE_PAUSE);
        assert_eq!(sends(&out), Some("Buy milk."));
    }

    #[test]
    fn cancel_drops_the_turn_and_keeps_listening() {
        let t = Instant::now();
        let (mut live, _) = Live::listening(EndMode::Pause, t);
        says(&mut live, "Text Sam that I'm late.", at(t, 100));
        says(&mut live, " Cancel.", at(t, 200));
        let out = live.input(Input::Tick, at(t, 200) + PHRASE_PAUSE);
        assert!(out.contains(&Effect::Toast(Toast::Cancelled)));
        assert_eq!(live.text(), "");
        assert_eq!(live.phase(), Phase::Listening);
        // Nothing left, so the pause sends nothing.
        assert_eq!(sends(&live.input(Input::Tick, at(t, 10_000))), None);
    }

    #[test]
    fn scratch_that_does_not_send_the_rest_until_more_is_said() {
        let t = Instant::now();
        let (mut live, _) = Live::listening(EndMode::Pause, t);
        says(&mut live, "Book a table for four.", at(t, 100));
        live.input(Input::Tick, at(t, 400));
        says(
            &mut live,
            " Make it the Italian place. Scratch that.",
            at(t, 450),
        );
        let out = live.input(Input::Tick, at(t, 450) + PHRASE_PAUSE);
        assert!(out.contains(&Effect::Toast(Toast::Scratched)));
        assert_eq!(live.text(), "Book a table for four.");
        assert_eq!(sends(&live.input(Input::Tick, at(t, 9_000))), None);
        says(&mut live, " At eight.", at(t, 9_000));
        let out = live.input(Input::Tick, at(t, 9_000) + PAUSE_SEND);
        assert_eq!(sends(&out), Some("Book a table for four. At eight."));
    }

    #[test]
    fn new_chat_marks_the_next_message() {
        let t = Instant::now();
        let (mut live, _) = Live::listening(EndMode::Word, t);
        says(&mut live, "New chat.", at(t, 100));
        let out = live.input(Input::Tick, at(t, 100) + PHRASE_PAUSE);
        assert!(out.contains(&Effect::Toast(Toast::NewChat)));
        says(&mut live, "Hello send", at(t, 3000));
        let out = live.input(Input::Tick, at(t, 3000) + PHRASE_PAUSE);
        assert!(out.contains(&Effect::Send {
            text: "Hello".into(),
            new_chat: true
        }));
    }

    #[test]
    fn speaking_done_goes_back_to_listening() {
        let t = Instant::now();
        let (mut live, _) = Live::listening(EndMode::Pause, t);
        says(&mut live, "Hi", at(t, 100));
        live.input(Input::Tick, at(t, 100) + PAUSE_SEND);
        assert_eq!(live.phase(), Phase::Thinking);
        assert_eq!(
            live.input(Input::ReplyReady, at(t, 4000)),
            vec![Effect::Speak, Effect::Transcribe]
        );
        assert_eq!(live.phase(), Phase::Speaking);
        assert_eq!(
            live.input(Input::SpeakDone, at(t, 9000)),
            vec![Effect::Transcribe]
        );
        assert_eq!(live.phase(), Phase::Listening);
    }

    #[test]
    fn a_tap_interrupts_speech() {
        let t = Instant::now();
        let (mut live, _) = Live::speaking(EndMode::Pause, t);
        let out = live.input(Input::Tap, at(t, 500));
        assert_eq!(
            out,
            vec![
                Effect::StopSpeaking,
                Effect::Transcribe,
                Effect::Toast(Toast::Stopped)
            ]
        );
        assert_eq!(live.phase(), Phase::Listening);
    }

    #[test]
    fn saying_stop_interrupts_speech_but_other_words_do_not() {
        let t = Instant::now();
        let (mut live, _) = Live::speaking(EndMode::Pause, t);
        // Beep's own voice leaking through is ignored.
        says(&mut live, "two emails need a reply", at(t, 100));
        assert!(
            live.input(Input::Tick, at(t, 100) + PHRASE_PAUSE)
                .is_empty()
        );
        assert_eq!(live.phase(), Phase::Speaking);
        says(&mut live, "Stop the server", at(t, 2000));
        assert!(
            live.input(Input::Tick, at(t, 2000) + PHRASE_PAUSE)
                .is_empty()
        );
        says(&mut live, "Stop.", at(t, 4000));
        let out = live.input(Input::Tick, at(t, 4000) + PHRASE_PAUSE);
        assert!(out.contains(&Effect::StopSpeaking));
        assert_eq!(live.phase(), Phase::Listening);
        // What was heard while speaking is not part of the next message.
        assert_eq!(live.text(), "");
    }

    #[test]
    fn tap_or_escape_leaves_and_keeps_a_draft() {
        let t = Instant::now();
        let (mut live, _) = Live::listening(EndMode::Word, t);
        assert_eq!(
            live.input(Input::Tap, at(t, 100)),
            vec![
                Effect::EndTranscription,
                Effect::Leave {
                    draft: None,
                    why: Leaving::Asked
                }
            ]
        );
        assert!(live.is_over());
        assert!(live.input(Input::Tap, at(t, 200)).is_empty());

        let (mut live, _) = Live::listening(EndMode::Word, t);
        says(&mut live, "Remind me at nine", at(t, 100));
        let out = live.input(Input::Escape, at(t, 300));
        assert!(out.contains(&Effect::Leave {
            draft: Some("Remind me at nine".into()),
            why: Leaving::Asked
        }));
    }

    #[test]
    fn bye_leaves() {
        let t = Instant::now();
        let (mut live, _) = Live::listening(EndMode::Pause, t);
        says(&mut live, "Bye!", at(t, 100));
        let out = live.input(Input::Tick, at(t, 100) + PHRASE_PAUSE);
        assert!(out.contains(&Effect::Leave {
            draft: None,
            why: Leaving::Asked
        }));
    }

    #[test]
    fn two_quiet_minutes_end_live_mode() {
        let t = Instant::now();
        let (mut live, _) = Live::listening(EndMode::Word, t);
        says(&mut live, "Something half", at(t, 100));
        assert!(live.input(Input::Tick, at(t, 60_000)).is_empty());
        let out = live.input(Input::Tick, at(t, 100) + QUIET_LIMIT);
        assert!(out.contains(&Effect::Leave {
            draft: Some("Something half".into()),
            why: Leaving::Quiet
        }));
    }

    #[test]
    fn holding_sends_on_release_and_another_key_cancels() {
        let t = Instant::now();
        let (mut live, _) = Live::holding(EndMode::Word, t);
        says(&mut live, "What's on tomorrow", at(t, 300));
        // Pauses while holding never send, even in pause mode.
        live.set_mode(EndMode::Pause);
        assert!(live.input(Input::Tick, at(t, 5000)).is_empty());
        let out = live.input(Input::HoldEnd, at(t, 5100));
        assert_eq!(sends(&out), Some("What's on tomorrow"));

        let (mut live, _) = Live::holding(EndMode::Word, t);
        says(&mut live, "Never", at(t, 300));
        let out = live.input(Input::HoldCancel, at(t, 400));
        assert!(out.contains(&Effect::Toast(Toast::Cancelled)));
        assert_eq!(live.phase(), Phase::Listening);
    }

    #[test]
    fn holding_while_speaking_interrupts_and_listens() {
        let t = Instant::now();
        let (mut live, _) = Live::speaking(EndMode::Pause, t);
        let out = live.input(Input::HoldStart, at(t, 100));
        assert_eq!(out, vec![Effect::StopSpeaking, Effect::Transcribe]);
        assert_eq!(live.phase(), Phase::Holding);
    }

    #[test]
    fn a_tap_while_thinking_skips_the_spoken_answer() {
        let t = Instant::now();
        let (mut live, _) = Live::listening(EndMode::Word, t);
        says(&mut live, "Hi send", at(t, 100));
        live.input(Input::Tick, at(t, 100) + PHRASE_PAUSE);
        assert_eq!(live.phase(), Phase::Thinking);
        assert_eq!(
            live.input(Input::Tap, at(t, 2000)),
            vec![Effect::Transcribe]
        );
        assert_eq!(live.phase(), Phase::Listening);
        assert!(live.input(Input::ReplyReady, at(t, 3000)).is_empty());
    }

    #[test]
    fn repeat_reads_the_last_answer_again() {
        let t = Instant::now();
        let (mut live, _) = Live::listening(EndMode::Pause, t);
        says(&mut live, "Repeat that.", at(t, 100));
        let out = live.input(Input::Tick, at(t, 100) + PHRASE_PAUSE);
        assert!(out.contains(&Effect::Repeat));
        assert_eq!(live.phase(), Phase::Speaking);
    }

    #[test]
    fn a_reply_with_nothing_to_say_goes_back_to_listening() {
        let t = Instant::now();
        let (mut live, _) = Live::listening(EndMode::Word, t);
        live.input(Input::Words("what time is it send".into()), t);
        let out = live.input(Input::Tick, at(t, 1300));
        assert_eq!(sends(&out), Some("what time is it"));
        assert_eq!(live.phase(), Phase::Thinking);
        assert_eq!(
            live.input(Input::SpeakDone, at(t, 4000)),
            vec![Effect::Transcribe]
        );
        assert_eq!(live.phase(), Phase::Listening);
    }
}
