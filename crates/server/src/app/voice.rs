//! Voice notes for a user: notices read out and spoken replies transcribed.

use super::App;
use crate::{
    protocol::{ApiError, Upload, VoiceNote},
    stt::{Session, SttError},
    user::User,
    voice::{self, VoiceError},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Live transcriptions the whole server runs at once.
const MAX_LIVE: usize = 16;
/// Live transcriptions one user runs at once.
const MAX_LIVE_PER_USER: usize = 2;

/// The name and type a voice note's audio goes out under.
const NOTE_NAME: &str = "voice-note.mp3";
const NOTE_TYPE: &str = "audio/mpeg";

impl App {
    /// `title` and `body` rewritten for listening and spoken.
    pub async fn speak_notice(
        &self,
        user: &User,
        title: &str,
        body: &str,
    ) -> Result<VoiceNote, ApiError> {
        let text = voice::spoken_text(&self.generator(user.id()), title, body).await;
        if text.is_empty() {
            return Err(ApiError::bad_request("nothing to say"));
        }
        let audio = self.voice.speak(user.id(), &text).await.map_err(refused)?;
        Ok(VoiceNote {
            text,
            audio: Upload {
                name: NOTE_NAME.to_owned(),
                mime: NOTE_TYPE.to_owned(),
                data: audio,
            },
        })
    }

    /// A line of live mode spoken as it is.
    pub async fn say(&self, user: &User, text: &str) -> Result<VoiceNote, ApiError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(ApiError::bad_request("nothing to say"));
        }
        let audio = self.voice.say(user.id(), text).await.map_err(refused)?;
        Ok(VoiceNote {
            text: text.to_owned(),
            audio: Upload {
                name: NOTE_NAME.to_owned(),
                mime: NOTE_TYPE.to_owned(),
                data: audio.as_ref().clone(),
            },
        })
    }

    /// What the user said in `audio`.
    pub async fn transcribe(&self, user: &User, audio: &Upload) -> Result<String, ApiError> {
        self.voice
            .transcribe(user.id(), audio)
            .await
            .map_err(refused)
    }
}

/// The live transcriptions running, server-wide and per user.
pub(super) struct LiveSlots {
    all: Arc<Semaphore>,
    users: Arc<Mutex<HashMap<String, usize>>>,
}

impl Default for LiveSlots {
    fn default() -> Self {
        Self {
            all: Arc::new(Semaphore::new(MAX_LIVE)),
            users: Arc::default(),
        }
    }
}

impl LiveSlots {
    /// A slot for `user`, unless the server or the user is at the limit.
    fn take(&self, user: &str) -> Option<LiveSlot> {
        let all = self.all.clone().try_acquire_owned().ok()?;
        let mut users = self.users.lock().unwrap_or_else(PoisonError::into_inner);
        let running = users.entry(user.to_owned()).or_default();
        if *running >= MAX_LIVE_PER_USER {
            return None;
        }
        *running += 1;
        Some(LiveSlot {
            _all: all,
            users: self.users.clone(),
            user: user.to_owned(),
        })
    }
}

/// One running live transcription's place, given back when dropped.
struct LiveSlot {
    _all: OwnedSemaphorePermit,
    users: Arc<Mutex<HashMap<String, usize>>>,
    user: String,
}

impl Drop for LiveSlot {
    fn drop(&mut self) {
        let mut users = self.users.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(running) = users.get_mut(&self.user) {
            *running = running.saturating_sub(1);
            if *running == 0 {
                users.remove(&self.user);
            }
        }
    }
}

/// A live transcription, and the slot it holds until dropped.
pub struct LiveTranscription {
    pub session: Session,
    _slot: LiveSlot,
}

impl App {
    /// Starts a live transcription for `user`, when the server has one and
    /// both the server and the user have a slot free.
    pub async fn transcribe_live(&self, user: &str) -> Result<LiveTranscription, SttError> {
        let slot = self.live_transcriptions.take(user).ok_or(SttError::Busy)?;
        let session = self.realtime.open().await?;
        Ok(LiveTranscription {
            session,
            _slot: slot,
        })
    }
}

/// What the app is told about a failed voice call.
fn refused(error: VoiceError) -> ApiError {
    match error {
        VoiceError::RateLimited | VoiceError::BadAudio | VoiceError::Silent => {
            ApiError::bad_request(error.to_string())
        }
        VoiceError::Busy | VoiceError::Provider => ApiError::Internal {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_user_runs_two_live_transcriptions_at_most() {
        let slots = LiveSlots::default();
        let first = slots.take("alice").expect("a first slot");
        let _second = slots.take("alice").expect("a second slot");
        assert!(slots.take("alice").is_none(), "a third waits");
        let _theirs = slots.take("bob").expect("another user has their own");
        drop(first);
        assert!(slots.take("alice").is_some(), "an ended one frees its slot");
    }
}
