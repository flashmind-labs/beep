//! A phone has no tray: only what the rest of the app expects of it.

use crate::beep::{Beep, state::LinkState};
use beep_proto::Session;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

/// The latest chats; nothing shows them on a phone.
#[derive(Default)]
pub(crate) struct Recent(#[allow(dead_code)] Mutex<Vec<Session>>);

pub(crate) fn install(_: &tauri::App, _: &Beep) -> tauri::Result<()> {
    Ok(())
}

pub(crate) fn refresh(_: &AppHandle) {}

pub(crate) async fn refresh_recent(_: AppHandle) {}

/// Tells the window this device now signs into an account.
pub(crate) fn became_linked(app: &AppHandle) {
    let _ = app.emit("link-state", LinkState::Linked);
    app.state::<crate::machines::this_mac::Machine>()
        .sync(&app.state::<Arc<Beep>>());
    crate::fleet::migrate::start(app.clone());
    app.state::<super::waiting::WaitingList>().poke();
}
