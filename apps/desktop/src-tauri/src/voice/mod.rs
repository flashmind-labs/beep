//! Live voice: live mode in the bar and the orb, push to talk, voice notes
//! and the notice card. `beep_live::Live` decides; `control` runs its
//! effects, with sound from `audio` and words from the server.

mod approvals;
mod asks;
pub(crate) mod audio;
mod card;
mod control;
mod event;
pub(crate) mod hotkey;
mod keys;
pub(crate) mod news;
mod note;
pub(crate) mod orb;
pub(crate) mod presence;
mod reader;
mod reply;
mod session;
mod transcribe;

use crate::{bar, beep::Beep};
use event::LiveEvent;
use session::State;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, atomic::AtomicU64};
use tauri::{AppHandle, Emitter};

/// Whether live mode and the orb exist. Off, Option never starts live mode and the orb window never
/// opens. The code stays.
pub(crate) const LIVE_ENABLED: bool = false;

pub use approvals::{Answered, Waiting};
pub use card::NoticeCard;

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
}
