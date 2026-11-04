//! One live session from start to leaving: its state, the audio engine it
//! opens, and what the user is told when it ends.

use super::{
    Voice,
    asks::Asks,
    audio::{Audio, MicChunk, MicDenied, Played},
    card,
    event::LiveEvent,
    hotkey::{self, Taps},
    news::Heard,
    orb,
    reader::Reader,
};
use crate::bar;
use beep_live::{Input, Leaving, Live};
use beep_proto::{Notice, VoiceNote};
use std::{
    sync::{Arc, atomic::Ordering},
    time::Instant,
};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::AbortHandle,
};
use uuid::Uuid;

/// Microphone stretches waiting to go up, about six seconds' worth: enough
/// to cover opening the transcription.
pub(super) const MIC_CHUNKS: usize = 64;
pub(super) const UNAVAILABLE: &str = "Voice is not available right now. Try again in a moment.";
const MIC_DENIED: &str =
    "Beep cannot hear you. Allow it in System Settings, Privacy & Security, Microphone.";
const QUIET: &str = "Live mode ended after 2 minutes quiet.";

/// How live mode starts.
pub(super) enum Start {
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
pub(super) struct Opening {
    heard: mpsc::Receiver<MicChunk>,
    audio: watch::Receiver<Option<Arc<Audio>>>,
    failed: oneshot::Receiver<OpenFailure>,
}

/// Opens the engine off the async workers. Without `listen` it only plays,
/// which needs no microphone permission. Dropping the `Opening` closes it.
pub(super) fn open_audio(listen: bool) -> Opening {
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
pub(super) struct Playing {
    pub(super) since: Instant,
    pub(super) played: Played,
}

/// One live session, from start to leaving.
pub(super) struct Session {
    pub(super) id: u64,
    pub(super) live: Live,
    pub(super) chat: Option<Uuid>,
    /// The chat before the last message, gone back to when it was not sent.
    pub(super) chat_before: Option<Uuid>,
    pub(super) title: Option<String>,
    /// A voice note read out: no transcription, and it ends when read.
    pub(super) note: bool,
    /// The notice a voice note reads, shown as a banner when it cannot be.
    pub(super) notice: Option<Box<Notice>>,
    /// The engine once it opened.
    pub(super) audio: watch::Receiver<Option<Arc<Audio>>>,
    /// Microphone sound for the transcription that runs.
    pub(super) stt: Option<mpsc::Sender<Vec<u8>>>,
    /// Counts transcriptions, so words from an ended one are dropped.
    pub(super) stt_run: u64,
    pub(super) stt_failures: u32,
    /// Tries while the server had no transcription free.
    pub(super) stt_busy: u32,
    /// The transcription whose last words a released hold waits for.
    pub(super) finishing: Option<u64>,
    pub(super) failure: Option<String>,
    /// Counts messages sent, so a reply that ended late changes nothing.
    pub(super) reply: u64,
    /// The run answering the last message, once it started.
    pub(super) run: Option<String>,
    /// That run is over: there is nothing left to cancel.
    pub(super) run_done: bool,
    /// The reply's text so far, for "repeat that".
    pub(super) reply_text: String,
    /// An answer given while the reply was still read out, to follow once
    /// what came before the approval is said.
    pub(super) resume: Option<Heard>,
    /// Reading a reply or a note aloud.
    pub(super) reader: Reader,
    pub(super) playing: Option<Playing>,
    pub(super) mic_level: f32,
    /// What the user sent last, so fillers are in its language.
    pub(super) said: String,
    /// The sentences said aloud last, for "repeat that" when the reply's
    /// text is not known.
    pub(super) last_reply: Vec<String>,
    pub(super) line: Option<String>,
    pub(super) prev: Option<String>,
    pub(super) step: Option<String>,
    /// The approvals waiting, for an orb opened after they came and for
    /// their notifications if live mode ends first.
    pub(super) asks: Asks,
    /// The sheet of what to say is open.
    pub(super) sheet: bool,
    pub(super) tasks: Vec<AbortHandle>,
}

impl Session {
    /// Stops anything being read out.
    pub(super) fn hush(&mut self) {
        self.reader.stop();
        if let Some(audio) = self.audio.borrow().as_ref() {
            audio.stop_playing();
        }
        self.playing = None;
        self.step = None;
    }
}

#[derive(Default)]
pub(super) struct State {
    pub(super) taps: Taps,
    pub(super) hold: hotkey::Hold,
    pub(super) live: Option<Session>,
    /// The microphone opened when Option went down in the open bar, for
    /// the press it went down for, so a hold hears its first word.
    pub(super) warm: Option<(u64, Opening)>,
    /// The new notice the orb's card offers to read aloud.
    pub(super) card: Option<card::Pending>,
}

impl Voice {
    pub(super) fn start(self: &Arc<Self>, how: Start, chat: Option<Uuid>) {
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
                        Ok(OpenFailure::Other(why)) => me.fail(id, UNAVAILABLE, why),
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
                asks: Asks::default(),
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

    /// Leaves live mode, keeping what was said and telling the user
    /// `failure`. `why` is only logged.
    pub(super) fn fail(self: &Arc<Self>, id: u64, failure: &str, why: String) {
        log::warn!("live mode is unavailable: {why}");
        if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
            s.failure = Some(failure.into());
        }
        self.input(id, Input::End(Leaving::Failed));
    }

    pub(super) fn leave(&self, id: u64, draft: Option<String>, why: Leaving) {
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
        // Approvals still waiting show as their notifications now, unless
        // the bar stays up on their chat, whose card asks.
        let asks = std::mem::take(&mut session.asks);
        // Dropping the session closes the microphone.
        drop(session);
        let shown = reopen.then(|| self.beep.session()).flatten();
        for notice in asks.into_notices() {
            if shown.as_deref() != Some(notice.session_id.as_str()) {
                crate::notices::notify::notice(&self.app, &notice);
            }
        }
        if reopen {
            bar::show(&self.app, None);
        }
    }

    /// Live mode in the orb ended on its own: a banner says why, since the
    /// orb is gone. A voice note that could not play shows as its banner.
    fn notify_left(&self, session: &Session, failure: Option<&str>, why: Leaving, kept: bool) {
        if let (Some(notice), Some(_)) = (session.notice.as_deref(), failure) {
            return crate::notices::notify::notice(&self.app, notice);
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
        crate::notices::notify::text(&self.app, "Live mode", &body);
    }

    /// The engine of session `id`, once it opened.
    pub(super) async fn audio(&self, id: u64) -> Option<Arc<Audio>> {
        let mut audio = {
            let state = self.state();
            state.live.as_ref().filter(|s| s.id == id)?.audio.clone()
        };
        let opened = audio.wait_for(Option::is_some).await.ok()?;
        opened.clone()
    }
}
