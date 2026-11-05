//! Signing out of the Beep server and back in, from Settings. A Mac signed
//! out keeps its key and its link, makes no server calls and takes no
//! notices; signing in is the same account again.

use crate::{
    beep::{Beep, Service, state::LinkState},
    error::Result,
    machines::this_mac::Machine,
    palette::PaletteCache,
    tray,
    voice::Voice,
};
use beep_proto::ListDevices;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

/// Stops using the server. Coding agents, machines, projects and the mesh
/// carry on; the assistant and voice go off.
#[tauri::command]
pub async fn sign_out(app: AppHandle, beep: Service<'_>) -> Result<()> {
    beep.sign_out().await;
    let voice = app.state::<Arc<Voice>>();
    if voice.is_live() {
        voice.leave_live();
    }
    app.state::<Machine>().sync(&beep);
    app.state::<PaletteCache>().link_changed();
    // Only the open webviews listen; there is nothing to do if none does.
    let _ = app.emit("signed-out", true);
    let _ = app.emit("link-state", beep.link_state());
    tray::refresh(&app);
    Ok(())
}

/// Connects again with the kept key. What the server has comes back through
/// the sync that follows the link, with no restart. A server that does not
/// answer leaves this Mac signed out, and says why.
#[tauri::command]
pub async fn sign_in(app: AppHandle, beep: Service<'_>) -> Result<()> {
    resume(&app, &beep).await
}

/// `sign_in`, for the flows that start an account on this Mac.
pub(super) async fn resume(app: &AppHandle, beep: &Beep) -> Result<()> {
    if !beep.signed_out() {
        return Ok(());
    }
    beep.set_signed_out(false);
    if let Err(e) = beep.call(ListDevices {}).await {
        // A key the server dropped is not signed out, it is removed.
        if beep.link_state() == LinkState::Revoked {
            let _ = app.emit("link-state", LinkState::Revoked);
        } else {
            beep.set_signed_out(true);
        }
        return Err(e);
    }
    let _ = app.emit("signed-out", false);
    app.state::<PaletteCache>().link_changed();
    tray::became_linked(app);
    Ok(())
}
