//! The connection to the Beep server (`Beep`), the runs it streams, signing in,
//! linking to a phone, this Mac's key and saved state, and the local copy of chats.

mod client;
pub(crate) mod devices;
pub(crate) mod identity;
mod link;
pub(crate) mod login;
mod run;
pub(crate) mod signout;
pub(crate) mod state;
pub(crate) mod sync;

pub use crate::chat::history::Turn;
pub use client::{AsyncMutex, Beep, Notices};
pub use link::{LinkView, qr_svg};
pub use run::{
    Asking, BarEvent, DataImage, Decision, Field, FileUpload, Pick, RunNews, Sent, chat_id,
    image_uris, session_uuid,
};

use std::{sync::Arc, time::SystemTime};
use tauri::State;

/// Whether the desktop talks to a Beep server at all. Off, no connection is made and no server
/// is chosen, whatever the saved state or the build says, and the window hides what the server
/// gives (`Status::server`). The code stays, and so do the key and the saved link: turning it on
/// brings the account back. This is the one place that decides it.
pub(crate) const SERVER_ENABLED: bool = false;

/// The connection, as a command takes it.
pub type Service<'a> = State<'a, Arc<Beep>>;

/// `at` in milliseconds since the Unix epoch, as the bar's page reads times.
/// A time before the epoch reads as the epoch.
pub fn epoch_ms(at: SystemTime) -> u64 {
    at.duration_since(SystemTime::UNIX_EPOCH)
        .map(|since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}
