//! Live voice: live mode in the bar and the orb, push to talk, voice notes
//! and the notice card. `beep_live::Live` decides; `control` runs its
//! effects, with sound from `audio` and words from the server.

mod approvals;
mod asks;
pub(crate) mod audio;
mod card;
mod control;
mod dictation;
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
mod typing;

use crate::{
    bar,
    beep::{Beep, identity::data_dir},
};
use beep_dictation::{Settings, load_settings};
use event::LiveEvent;
use session::State;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, atomic::AtomicU64};
use tauri::{AppHandle, Emitter};

pub use approvals::{Answered, Waiting};
pub use card::NoticeCard;
pub(crate) use dictation::api_provider;

pub struct Voice {
    app: AppHandle,
    beep: Arc<Beep>,
    state: Mutex<State>,
    /// Counts live sessions.
    lives: AtomicU64,
    /// Counts audio files, so each sentence has its own.
    files: AtomicU64,
    /// What the user picked for dictation. With no provider it is off.
    dictation: Mutex<Settings>,
    /// Counts dictation's recordings.
    recordings: AtomicU64,
    /// Counts what dictation shows in the orb, so a late hide spares a newer one.
    dictations: AtomicU64,
}

impl Voice {
    pub fn new(app: AppHandle, beep: Arc<Beep>) -> Arc<Self> {
        let settings = data_dir(&app)
            .map(|dir| tauri::async_runtime::block_on(load_settings(&dir)))
            .unwrap_or_default();
        Arc::new(Self {
            app,
            beep,
            state: Mutex::new(State::default()),
            lives: AtomicU64::new(0),
            files: AtomicU64::new(0),
            dictation: Mutex::new(settings),
            recordings: AtomicU64::new(0),
            dictations: AtomicU64::new(0),
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
