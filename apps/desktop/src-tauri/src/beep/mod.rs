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

use anyhow::Context;
use iroh::EndpointId;
use std::{sync::Arc, time::SystemTime};
use tauri::State;

/// Whether the desktop talks to a Beep server at all. Off, no connection is made and no server
/// is chosen, whatever the saved state or the build says, and the window hides what the server
/// gives (`Status::server`). The code stays, and so do the key and the saved link: turning it on
/// brings the account back. This is the one place that decides it.
pub(crate) const SERVER_ENABLED: bool = false;

/// The server to talk to: the one chosen with a link code, else `built_in` (empty for none), and
/// no server while `enabled` is off.
pub(crate) fn server_to_use(
    enabled: bool,
    chosen: Option<EndpointId>,
    built_in: &str,
) -> anyhow::Result<Option<EndpointId>> {
    if !enabled {
        return Ok(None);
    }
    match chosen {
        Some(server) => Ok(Some(server)),
        None if built_in.is_empty() => Ok(None),
        None => Ok(Some(
            built_in
                .parse()
                .context("the server node id built in is not valid")?,
        )),
    }
}

/// The connection, as a command takes it.
pub type Service<'a> = State<'a, Arc<Beep>>;

/// `at` in milliseconds since the Unix epoch, as the bar's page reads times.
/// A time before the epoch reads as the epoch.
pub fn epoch_ms(at: SystemTime) -> u64 {
    at.duration_since(SystemTime::UNIX_EPOCH)
        .map(|since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    #[test]
    fn the_switch_off_leaves_no_server_whatever_is_chosen_or_built_in() {
        let chosen = SecretKey::generate().public();
        let built_in = SecretKey::generate().public();
        let text = built_in.to_string();
        assert_eq!(server_to_use(false, Some(chosen), &text).expect("ok"), None);
        assert_eq!(
            server_to_use(true, Some(chosen), &text).expect("ok"),
            Some(chosen)
        );
        assert_eq!(
            server_to_use(true, None, &text).expect("ok"),
            Some(built_in)
        );
        assert_eq!(server_to_use(true, None, "").expect("ok"), None);
        assert!(server_to_use(true, None, "not an id").is_err());
    }
}
