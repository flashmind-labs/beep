//! Live mode as a state machine: listening, thinking, speaking and back to
//! listening. It only decides; `voice` opens the microphone, talks to the
//! server and plays the audio for each `Effect`.

use crate::{
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
    /// The message went; waiting for the answer. Still listening, but only
    /// for "stop" and "bye".
    Thinking,
    Speaking,
    /// The reply stopped on a tool call that waits for the user's OK, given
    /// on screen. Listening as in `Listening`, but quiet never ends live
    /// mode.
    Waiting,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Listening => "listening",
            Self::Holding => "holding",
            Self::Thinking => "thinking",
            Self::Speaking => "speaking",
            Self::Waiting => "waiting",
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
    /// The message never reached the server: its words come back, to send
    /// again.
    NotSent,
    /// Beep finished speaking.
    SpeakDone,
    /// The reply stopped on a tool call that waits for the user's OK, and
    /// everything before it was read out.
    Approval,
    /// The approval the reply waits on was answered: the reply goes on.
    Answered,
    /// Live mode ends for `Leaving`, such as a call taking the audio.
    End(Leaving),
    /// Esc or Cancel: stops the reply Beep thinks about or speaks and
    /// listens again, with the message that asked for it back in the
    /// words. With no reply going on it leaves, like `Escape`.
    Cancel,
}

/// A short note the bar shows when a command was heard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toast {
    Cancelled,
    Scratched,
    NewChat,
    Stopped,
    Repeating,
    /// "stop" while nothing plays.
    NothingPlaying,
}

impl Toast {
    pub fn text(self) -> &'static str {
        match self {
            Self::Cancelled => "Cancelled",
            Self::Scratched => "Removed last sentence",
            Self::NewChat => "New chat",
            Self::Stopped => "Stopped",
            Self::Repeating => "Repeating",
            Self::NothingPlaying => "Nothing playing · say bye to leave",
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
    /// Another app took the audio, such as a phone call.
    Interrupted,
    /// Live mode could not go on, such as without the microphone.
    Failed,
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
    /// Stops the reply on the server and drops the rest of its events: the
    /// user interrupted it or left while Beep was thinking or speaking. The
    /// run may have finished already, which makes it a no-op.
    CancelReply,
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
            Self::CancelReply => "cancel reply",
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
    /// Words heard while Beep thinks or speaks: only a command counts there,
    /// since anything else may be Beep's own voice.
    barge: Turn,
    /// The last message sent, put back when its reply fails or is
    /// cancelled.
    sent: String,
    /// Started by holding Option: between turns nothing listens, and the
    /// next hold talks.
    hold_only: bool,
    /// Last sound or word, for pauses.
    active_at: Instant,
    /// Last word, or the start of listening, for `QUIET_LIMIT`.
    heard_at: Instant,
    new_chat: bool,
    /// Whether a pause may send: off after a command until new words.
    armed: bool,
    soon: bool,
    /// The reply waits on an approval: listening goes to `Waiting`.
    approval: bool,
    over: bool,
}

impl Live {
    fn new(phase: Phase, mode: EndMode, now: Instant) -> Self {
        Self {
            phase,
            mode,
            turn: Turn::default(),
            barge: Turn::default(),
            sent: String::new(),
            hold_only: false,
            active_at: now,
            heard_at: now,
            new_chat: false,
            armed: true,
            soon: false,
            approval: false,
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

    /// Live mode started by holding Option, as push to talk: once Beep has
    /// answered, transcription ends until the next hold, so nothing said
    /// between holds is sent.
    pub fn hold_to_talk(mode: EndMode, now: Instant) -> (Self, Vec<Effect>) {
        let mut live = Self::new(Phase::Holding, mode, now);
        live.hold_only = true;
        (live, vec![Effect::Transcribe])
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

    /// Whether live mode only listens while Option is held.
    pub fn hold_only(&self) -> bool {
        self.hold_only
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
            (_, Input::End(why)) => self.leave(why),
            (Phase::Thinking | Phase::Speaking, Input::Cancel) => self.cancel_reply(now),
            (_, Input::Cancel) => self.leave(Leaving::Asked),
            (Phase::Listening | Phase::Holding | Phase::Waiting, Input::Words(delta)) => {
                self.turn.hear(&delta);
                self.active_at = now;
                self.heard_at = now;
                self.armed = true;
                let mut out = self.not_soon();
                out.push(Effect::Show);
                out
            }
            (Phase::Thinking | Phase::Speaking, Input::Words(delta)) => {
                self.barge.hear(&delta);
                self.active_at = now;
                Vec::new()
            }
            (_, Input::Sound) => {
                self.active_at = now;
                self.not_soon()
            }
            (Phase::Listening | Phase::Waiting, Input::Tick) => self.listening_tick(now),
            (Phase::Thinking | Phase::Speaking, Input::Tick) => self.barge_tick(now),
            (Phase::Listening | Phase::Waiting, Input::Tap) => self.leave(Leaving::Asked),
            (Phase::Listening | Phase::Waiting, Input::HoldStart) => {
                self.phase = Phase::Holding;
                let mut out = self.not_soon();
                // Between holds nothing listened.
                if self.hold_only {
                    out.push(Effect::Transcribe);
                }
                out
            }
            (Phase::Thinking | Phase::Speaking, Input::Tap) => {
                let mut out = self.interrupt(now);
                out.push(self.resume());
                out.push(Effect::Toast(Toast::Stopped));
                out
            }
            (Phase::Thinking | Phase::Speaking, Input::HoldStart) => {
                let mut out = self.interrupt(now);
                self.phase = Phase::Holding;
                out.push(Effect::Transcribe);
                out
            }
            (Phase::Thinking, Input::ReplyReady) => {
                self.phase = Phase::Speaking;
                self.barge.clear();
                vec![Effect::Speak, Effect::Transcribe]
            }
            (Phase::Thinking | Phase::Speaking, Input::NotSent) => {
                self.listen(now);
                self.restore();
                vec![self.resume(), Effect::Show]
            }
            // The reply ended: with nothing to say aloud, all of it said,
            // or failed.
            (Phase::Thinking | Phase::Speaking, Input::ReplyFailed | Input::SpeakDone) => {
                self.listen(now);
                vec![self.resume()]
            }
            (Phase::Thinking | Phase::Speaking, Input::Approval) => {
                self.approval = true;
                self.listen(now);
                vec![self.resume()]
            }
            (Phase::Waiting, Input::Answered) => {
                self.approval = false;
                self.phase = Phase::Thinking;
                self.barge.clear();
                let mut out = self.not_soon();
                out.push(Effect::Transcribe);
                out
            }
            // The user is talking: the reply goes on in the chat, unspoken.
            (Phase::Holding, Input::Answered) => {
                self.approval = false;
                Vec::new()
            }
            (Phase::Holding, Input::HoldEnd) => self.hold_end(),
            (Phase::Holding, Input::HoldCancel) => {
                self.turn.clear();
                self.phase = self.idle();
                vec![self.resume(), Effect::Toast(Toast::Cancelled), Effect::Show]
            }
            _ => Vec::new(),
        }
    }

    /// The phase that listens for a new turn.
    fn idle(&self) -> Phase {
        if self.approval {
            Phase::Waiting
        } else {
            Phase::Listening
        }
    }

    /// What the transcription does between turns: a fresh one, so nothing
    /// heard before counts, or none when only a hold listens.
    fn resume(&self) -> Effect {
        if self.hold_only {
            Effect::EndTranscription
        } else {
            Effect::Transcribe
        }
    }

    /// Option came up after a hold: a command said on its own acts, other
    /// words are sent. Either way what follows starts clean.
    fn hold_end(&mut self) -> Vec<Effect> {
        let closed = self.turn.close();
        self.phase = self.idle();
        let mut out = match closed {
            Closed::Command(command) => self.command(command),
            _ if self.turn.is_empty() => vec![Effect::Show],
            _ => return self.send(),
        };
        // Sending, repeating and leaving see to the transcription
        // themselves.
        let handled = out
            .iter()
            .any(|e| matches!(e, Effect::Transcribe | Effect::EndTranscription));
        if !handled {
            out.insert(0, self.resume());
        }
        out
    }

    /// Stops the reply Beep thinks about or speaks and goes back to the
    /// phase that listens. The caller says what the transcription does.
    fn interrupt(&mut self, now: Instant) -> Vec<Effect> {
        let mut out = Vec::new();
        if self.phase == Phase::Speaking {
            out.push(Effect::StopSpeaking);
        }
        out.push(Effect::CancelReply);
        self.listen(now);
        out
    }

    /// Stops the reply and puts the message that asked for it back.
    fn cancel_reply(&mut self, now: Instant) -> Vec<Effect> {
        let mut out = self.interrupt(now);
        self.restore();
        out.extend([self.resume(), Effect::Toast(Toast::Cancelled), Effect::Show]);
        out
    }

    /// Puts the last message sent back in front of the words, where a
    /// pause does not send it again until more is said.
    fn restore(&mut self) {
        let sent = std::mem::take(&mut self.sent);
        if !sent.is_empty() {
            self.turn.restore(&sent);
            self.armed = false;
        }
    }

    /// Back to listening for a new turn, with the pause clocks restarted.
    fn listen(&mut self, now: Instant) {
        self.phase = self.idle();
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
        if self.phase == Phase::Listening
            && now.saturating_duration_since(self.heard_at) >= QUIET_LIMIT
        {
            return self.leave(Leaving::Quiet);
        }
        Vec::new()
    }

    /// Checks what was said while Beep thinks or speaks for a command.
    /// Only whole phrases that are commands count, so Beep's own voice
    /// coming back does nothing.
    fn barge_tick(&mut self, now: Instant) -> Vec<Effect> {
        let quiet = now.saturating_duration_since(self.active_at);
        if !self.barge.is_open() || quiet < PHRASE_PAUSE {
            return Vec::new();
        }
        let closed = self.barge.close();
        self.barge.clear();
        match closed {
            Closed::Command(Command::Stop) => {
                let mut out = self.interrupt(now);
                out.push(self.resume());
                out.push(Effect::Toast(Toast::Stopped));
                out
            }
            _ => Vec::new(),
        }
    }

    /// A command said while the user talks: both end what they say and
    /// send it, so "stop" works as "send" does.
    fn command(&mut self, command: Command) -> Vec<Effect> {
        match command {
            Command::Send | Command::Stop => self.send(),
        }
    }

    fn send(&mut self) -> Vec<Effect> {
        let text = self.turn.text();
        let mut out = self.not_soon();
        if text.is_empty() {
            return out;
        }
        self.turn.clear();
        self.phase = Phase::Thinking;
        // A new message leaves an approval unanswered.
        self.approval = false;
        self.barge.clear();
        text.clone_into(&mut self.sent);
        // A fresh transcription, so what is heard while Beep thinks starts
        // clean: only commands count there.
        out.extend([
            Effect::Transcribe,
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
        // Leaving mid-reply stops it on the server too.
        if matches!(self.phase, Phase::Thinking | Phase::Speaking) {
            out.push(Effect::CancelReply);
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
                Effect::CancelReply,
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
        assert_eq!(
            out,
            vec![
                Effect::StopSpeaking,
                Effect::CancelReply,
                Effect::Transcribe
            ]
        );
        assert_eq!(live.phase(), Phase::Holding);
    }

    #[test]
    fn a_tap_while_thinking_cancels_the_reply() {
        let t = Instant::now();
        let (mut live, _) = Live::listening(EndMode::Word, t);
        says(&mut live, "Hi send", at(t, 100));
        live.input(Input::Tick, at(t, 100) + PHRASE_PAUSE);
        assert_eq!(live.phase(), Phase::Thinking);
        assert_eq!(
            live.input(Input::Tap, at(t, 2000)),
            vec![
                Effect::CancelReply,
                Effect::Transcribe,
                Effect::Toast(Toast::Stopped)
            ]
        );
        assert_eq!(live.phase(), Phase::Listening);
        assert!(live.input(Input::ReplyReady, at(t, 3000)).is_empty());
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

    /// Sends "Hi" in word mode, leaving `live` thinking.
    fn thinking(t: Instant) -> Live {
        let (mut live, _) = Live::listening(EndMode::Word, t);
        says(&mut live, "Hi send", at(t, 100));
        let out = live.input(Input::Tick, at(t, 100) + PHRASE_PAUSE);
        assert_eq!(sends(&out), Some("Hi"));
        // Still listening while Beep thinks, for "stop".
        assert!(out.contains(&Effect::Transcribe));
        assert!(!out.contains(&Effect::EndTranscription));
        live
    }

    #[test]
    fn saying_stop_while_thinking_cancels_the_reply() {
        let t = Instant::now();
        let mut live = thinking(t);
        // Other words while thinking are not a new message.
        says(&mut live, "and the weather", at(t, 2000));
        assert!(
            live.input(Input::Tick, at(t, 2000) + PHRASE_PAUSE)
                .is_empty()
        );
        assert_eq!(live.phase(), Phase::Thinking);
        says(&mut live, "Stop.", at(t, 5000));
        let out = live.input(Input::Tick, at(t, 5000) + PHRASE_PAUSE);
        assert_eq!(
            out,
            vec![
                Effect::CancelReply,
                Effect::Transcribe,
                Effect::Toast(Toast::Stopped)
            ]
        );
        assert_eq!(live.phase(), Phase::Listening);
        assert_eq!(live.text(), "");
    }

    #[test]
    fn saying_stop_while_speaking_cancels_the_reply() {
        let t = Instant::now();
        let mut live = thinking(t);
        live.input(Input::ReplyReady, at(t, 3000));
        says(&mut live, "Stop", at(t, 4000));
        let out = live.input(Input::Tick, at(t, 4000) + PHRASE_PAUSE);
        assert_eq!(
            out,
            vec![
                Effect::StopSpeaking,
                Effect::CancelReply,
                Effect::Transcribe,
                Effect::Toast(Toast::Stopped)
            ]
        );
    }

    #[test]
    fn leaving_mid_reply_cancels_it_but_not_after() {
        let t = Instant::now();
        let mut live = thinking(t);
        live.input(Input::ReplyReady, at(t, 3000));
        let out = live.input(Input::Escape, at(t, 3500));
        assert_eq!(
            out,
            vec![
                Effect::EndTranscription,
                Effect::StopSpeaking,
                Effect::CancelReply,
                Effect::Leave {
                    draft: None,
                    why: Leaving::Asked
                }
            ]
        );

        let (mut live, _) = Live::listening(EndMode::Word, t);
        let out = live.input(Input::End(Leaving::Interrupted), at(t, 100));
        assert!(!out.contains(&Effect::CancelReply));
        assert!(out.contains(&Effect::Leave {
            draft: None,
            why: Leaving::Interrupted
        }));
    }

    #[test]
    fn holding_while_thinking_cancels_and_listens() {
        let t = Instant::now();
        let mut live = thinking(t);
        let out = live.input(Input::HoldStart, at(t, 2000));
        assert_eq!(out, vec![Effect::CancelReply, Effect::Transcribe]);
        assert_eq!(live.phase(), Phase::Holding);
    }

    #[test]
    fn a_reply_failing_while_speaking_goes_back_to_listening() {
        let t = Instant::now();
        let mut live = thinking(t);
        live.input(Input::ReplyReady, at(t, 3000));
        assert_eq!(
            live.input(Input::ReplyFailed, at(t, 4000)),
            vec![Effect::Transcribe]
        );
        assert_eq!(live.phase(), Phase::Listening);
        // The message is in the chat already.
        assert_eq!(live.text(), "");
    }

    #[test]
    fn a_message_not_sent_comes_back_without_sending_again() {
        let t = Instant::now();
        let (mut live, _) = Live::listening(EndMode::Pause, t);
        says(&mut live, "Book the dentist", at(t, 100));
        let out = live.input(Input::Tick, at(t, 100) + PAUSE_SEND);
        assert_eq!(sends(&out), Some("Book the dentist"));
        assert_eq!(
            live.input(Input::NotSent, at(t, 3000)),
            vec![Effect::Transcribe, Effect::Show]
        );
        assert_eq!(live.text(), "Book the dentist");
        // A pause alone does not send it again.
        assert_eq!(sends(&live.input(Input::Tick, at(t, 9000))), None);
        says(&mut live, " for Friday", at(t, 9000));
        let out = live.input(Input::Tick, at(t, 9000) + PAUSE_SEND);
        assert_eq!(sends(&out), Some("Book the dentist for Friday"));
    }

    #[test]
    fn cancel_stops_the_reply_and_puts_the_message_back() {
        let t = Instant::now();
        let mut live = thinking(t);
        live.input(Input::ReplyReady, at(t, 3000));
        assert_eq!(
            live.input(Input::Cancel, at(t, 3500)),
            vec![
                Effect::StopSpeaking,
                Effect::CancelReply,
                Effect::Transcribe,
                Effect::Toast(Toast::Cancelled),
                Effect::Show,
            ]
        );
        assert_eq!(live.phase(), Phase::Listening);
        assert_eq!(live.text(), "Hi");
        assert!(!live.is_over());

        // With no reply going on, Cancel leaves like Escape.
        let (mut live, _) = Live::listening(EndMode::Word, t);
        says(&mut live, "Half a thought", at(t, 100));
        assert!(
            live.input(Input::Cancel, at(t, 200))
                .contains(&Effect::Leave {
                    draft: Some("Half a thought".into()),
                    why: Leaving::Asked
                })
        );
    }

    #[test]
    fn stop_while_talking_sends() {
        let t = Instant::now();
        let (mut live, _) = Live::listening(EndMode::Word, t);
        says(&mut live, "Book a table for four. Stop.", at(t, 100));
        let out = live.input(Input::Tick, at(t, 100) + PHRASE_PAUSE);
        assert_eq!(sends(&out), Some("Book a table for four."));

        // With nothing said, it sends nothing.
        let (mut live, _) = Live::listening(EndMode::Word, t);
        says(&mut live, "Stop.", at(t, 100));
        let out = live.input(Input::Tick, at(t, 100) + PHRASE_PAUSE);
        assert_eq!(sends(&out), None);
        assert_eq!(live.phase(), Phase::Listening);
    }

    #[test]
    fn letting_go_after_stop_sends() {
        let t = Instant::now();
        let (mut live, _) = Live::listening(EndMode::Word, t);
        live.input(Input::HoldStart, at(t, 100));
        says(&mut live, "Book a table. Stop.", at(t, 200));
        let out = live.input(Input::HoldEnd, at(t, 300));
        assert_eq!(sends(&out), Some("Book a table."));
    }

    #[test]
    fn an_empty_hold_sends_nothing_now_or_later() {
        let t = Instant::now();
        let (mut live, _) = Live::listening(EndMode::Pause, t);
        live.input(Input::HoldStart, at(t, 100));
        let out = live.input(Input::HoldEnd, at(t, 200));
        // A fresh transcription: late words of the hold do not count.
        assert_eq!(out, vec![Effect::Transcribe, Effect::Show]);
        assert_eq!(live.phase(), Phase::Listening);
    }

    #[test]
    fn hold_to_talk_listens_only_while_held() {
        let t = Instant::now();
        let (mut live, start) = Live::hold_to_talk(EndMode::Pause, t);
        assert_eq!(start, vec![Effect::Transcribe]);
        assert!(live.hold_only());
        says(&mut live, "What's on tomorrow", at(t, 300));
        let out = live.input(Input::HoldEnd, at(t, 600));
        assert_eq!(sends(&out), Some("What's on tomorrow"));
        // "stop" still works while Beep thinks and speaks.
        assert!(out.contains(&Effect::Transcribe));
        live.input(Input::ReplyReady, at(t, 2000));
        assert_eq!(
            live.input(Input::SpeakDone, at(t, 5000)),
            vec![Effect::EndTranscription]
        );
        assert_eq!(live.phase(), Phase::Listening);
        assert_eq!(
            live.input(Input::HoldStart, at(t, 8000)),
            vec![Effect::Transcribe]
        );
        assert_eq!(live.phase(), Phase::Holding);
        assert_eq!(
            live.input(Input::HoldEnd, at(t, 8300)),
            vec![Effect::EndTranscription, Effect::Show]
        );
        // A tap between holds leaves.
        assert!(
            live.input(Input::Tap, at(t, 9000))
                .contains(&Effect::Leave {
                    draft: None,
                    why: Leaving::Asked
                })
        );
    }

    #[test]
    fn a_tap_mid_reply_in_hold_to_talk_stops_listening() {
        let t = Instant::now();
        let (mut live, _) = Live::hold_to_talk(EndMode::Pause, t);
        says(&mut live, "Hi", at(t, 300));
        live.input(Input::HoldEnd, at(t, 600));
        assert_eq!(
            live.input(Input::Tap, at(t, 1000)),
            vec![
                Effect::CancelReply,
                Effect::EndTranscription,
                Effect::Toast(Toast::Stopped)
            ]
        );
    }

    #[test]
    fn an_approval_waits_without_the_quiet_limit_then_goes_on() {
        let t = Instant::now();
        let mut live = thinking(t);
        live.input(Input::ReplyReady, at(t, 3000));
        assert_eq!(
            live.input(Input::Approval, at(t, 4000)),
            vec![Effect::Transcribe]
        );
        assert_eq!(live.phase(), Phase::Waiting);
        for s in (5..=600).step_by(5) {
            assert!(live.input(Input::Tick, at(t, s * 1000)).is_empty());
        }
        assert_eq!(live.phase(), Phase::Waiting);
        assert_eq!(
            live.input(Input::Answered, at(t, 601_000)),
            vec![Effect::Transcribe]
        );
        assert_eq!(live.phase(), Phase::Thinking);
        live.input(Input::SpeakDone, at(t, 605_000));
        assert_eq!(live.phase(), Phase::Listening);
        // Answered, the quiet limit counts again.
        let out = live.input(Input::Tick, at(t, 605_000) + QUIET_LIMIT);
        assert!(out.contains(&Effect::Leave {
            draft: None,
            why: Leaving::Quiet
        }));
    }

    #[test]
    fn while_waiting_a_new_message_drops_the_approval() {
        let t = Instant::now();
        let mut live = thinking(t);
        live.input(Input::Approval, at(t, 3000));
        assert_eq!(live.phase(), Phase::Waiting);
        says(&mut live, "Actually do something else send", at(t, 9000));
        let out = live.input(Input::Tick, at(t, 9000) + PHRASE_PAUSE);
        assert_eq!(sends(&out), Some("Actually do something else"));
        assert!(live.input(Input::Answered, at(t, 12_000)).is_empty());
        live.input(Input::SpeakDone, at(t, 15_000));
        assert_eq!(live.phase(), Phase::Listening);
    }
}
