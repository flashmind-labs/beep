//! Live mode's controls and its loop: taps, Esc, the sheet, the inputs
//! `beep_live::Live` takes, the effects it asks for, and the orb's view.

use super::{Voice, audio::MicChunk, event::LiveEvent, hotkey, orb};
use crate::bar;
use beep_live::{Effect, EndMode, Input, Phase, Toast as LiveToast};
use beep_proto::CancelRun;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, time::interval};
use uuid::Uuid;

/// How often live mode checks pauses and moves the orb.
const TICK: Duration = Duration::from_millis(50);
/// Loudness above which the microphone counts as hearing someone.
const SOUND_LEVEL: f32 = 0.12;
const NOTHING_TO_REPEAT: &str = "Nothing to repeat";

impl Voice {
    pub(super) fn end_mode(&self) -> EndMode {
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
    pub(super) fn voice(&self) -> hotkey::Voice {
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
        for waiting in self.waiting() {
            self.emit(LiveEvent::Approval(waiting));
        }
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
        if self.cancel_dictation() {
            return;
        }
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
    pub(super) fn open_chat(&self) -> Option<Uuid> {
        if !bar::is_open(&self.app) {
            return None;
        }
        self.beep.session().and_then(|s| Uuid::parse_str(&s).ok())
    }

    /// Hands `input` to the live session running now.
    pub(super) fn input_now(self: &Arc<Self>, input: Input) {
        let id = self.state().live.as_ref().map(|s| s.id);
        if let Some(id) = id {
            self.input(id, input);
        }
    }

    /// Hands `input` to live session `id` and runs what it asks for.
    pub(super) fn input(self: &Arc<Self>, id: u64, input: Input) {
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

    pub(super) fn apply(self: &Arc<Self>, id: u64, effects: Vec<Effect>) {
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

    pub(super) fn show(&self, id: u64) {
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
                dictating: false,
            }
        };
        self.emit(view);
    }

    /// Forwards the microphone: to the transcription while one runs, as
    /// sound for pauses, and as the orb's level.
    pub(super) async fn pump(self: Arc<Self>, id: u64, mut heard: mpsc::Receiver<MicChunk>) {
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
    pub(super) async fn tick(self: Arc<Self>, id: u64) {
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

    pub(super) fn set_step(&self, id: u64, step: Option<String>) {
        if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
            s.step = step;
        }
        self.show(id);
    }
}
