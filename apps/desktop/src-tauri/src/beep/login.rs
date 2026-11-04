//! Signing this Mac in: through the website on a hosted server, or by choosing
//! the user's own server with its link code.

use crate::{
    bar::{self, app::open_url},
    beep::{Beep, Service, identity::computer_name, state::LinkState},
    error::{DesktopError, Result},
    tray,
};
use beep_proto::{
    ApiError, CompleteLogin, ListDevices, SecretText, StartLogin, link::LinkOffer,
    urls::login_token,
};
use std::{sync::Arc, time::SystemTime};
use tauri::{AppHandle, Emitter, Manager};

/// Whether the server wants a sign-in rather than today's link flows.
#[tauri::command]
pub async fn login_hosted(beep: Service<'_>) -> Result<bool> {
    Ok(beep.has_login().await)
}

/// Opens the browser at the website with a new code for this key.
#[tauri::command]
pub async fn start_login(beep: Service<'_>) -> Result<()> {
    let device = computer_name();
    let url = beep.guest_call(StartLogin { device }).await?;
    open_url(url.expose().to_owned())
}

/// Finishes the sign-in with a token pasted from the website.
#[tauri::command]
pub async fn complete_login(app: AppHandle, beep: Service<'_>, token: String) -> Result<()> {
    finish(&app, &beep, token).await
}

/// Takes a `beep://login` link: the same, then the bar shows how it went and
/// hears `login-failed` with why, if it did.
pub fn open_link(app: &AppHandle, url: &str) {
    let Some(token) = login_token(url) else {
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let beep = app.state::<Arc<Beep>>().inner().clone();
        let failed = finish(&app, &beep, token).await.err();
        bar::show(&app, Some("link"));
        if let Some(failed) = failed {
            let _ = app.emit_to("bar", "login-failed", failed.to_string());
        }
    });
}

async fn finish(app: &AppHandle, beep: &Beep, token: String) -> Result<()> {
    let token = SecretText::new(token.trim());
    beep.guest_call(CompleteLogin { token }).await?;
    if beep.link_state() != LinkState::Linked {
        // The guest connection is on its way out; the next call signs in.
        beep.forget_connection().await;
        beep.set_link_state(LinkState::Linked);
        tray::became_linked(app);
    }
    Ok(())
}

/// Chooses the server in a link code from `beep-server link`. The app restarts
/// on it, signed in when the server took this key, else at its link screens.
#[tauri::command]
pub async fn use_own_server(app: AppHandle, beep: Service<'_>, code: String) -> Result<()> {
    let offer = LinkOffer::parse(&code).map_err(|e| DesktopError::BadRequest(e.to_string()))?;
    let server = offer
        .server(SystemTime::now())
        .ok_or_else(|| DesktopError::BadRequest("that code expired or is not a server's".into()))?;
    let client = beep.connect_to(server).await?;
    let link = match client.call(&ListDevices {}).await {
        Ok(_) => LinkState::Linked,
        Err(e)
            if matches!(
                e.downcast_ref::<ApiError>(),
                Some(ApiError::Unauthorized {})
            ) =>
        {
            LinkState::Unlinked
        }
        Err(e) => return Err(e.into()),
    };
    beep.choose_server(server, link);
    app.restart()
}
