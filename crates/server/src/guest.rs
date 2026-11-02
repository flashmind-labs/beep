//! A key nobody linked, on a single-user server: it gets no account, only
//! the two calls that join the owner's with a code the owner's device
//! shows. Joining links the key to the owner's account and closes the
//! connection, so it signs in again as one of the owner's devices.

use crate::{
    app::App,
    protocol::{
        ApiError, ClientInfo, DeviceKind, JoinAccount, JoinPreview, PeekInvite, Platform, Rpc,
        link,
        wire::{Rx, Tx, framed},
    },
    router::read_request,
    store::NewKey,
};
use anyhow::Result;
use iroh::endpoint::{Connection, VarInt};
use std::{sync::Arc, time::Duration};
use tokio::time::sleep;

/// Calls one guest connection may make before it is closed.
const MAX_CALLS: usize = 16;
/// How long the joined key keeps its connection, so the reply gets there.
const REJOIN_AFTER: Duration = Duration::from_secs(1);
/// What the closed guest connection says.
const CLOSE_JOINED: u32 = 0;

/// Serves `connection`'s calls one at a time, after its `Hello`.
pub(crate) async fn serve(app: Arc<App>, connection: Connection, client: ClientInfo) {
    for _ in 0..MAX_CALLS {
        let Ok((send, recv)) = connection.accept_bi().await else {
            return;
        };
        let (mut tx, mut rx) = framed(send, recv);
        match call(&app, &connection, &client, &mut tx, &mut rx).await {
            Ok(true) => {
                sleep(REJOIN_AFTER).await;
                connection.close(VarInt::from_u32(CLOSE_JOINED), b"joined");
                return;
            }
            Ok(false) => {}
            Err(e) => log::debug!("guest stream ended: {e:#}"),
        }
    }
    connection.close(VarInt::from_u32(CLOSE_JOINED), b"too many calls");
}

/// Answers one call; true once the key joined the owner's account.
async fn call(
    app: &App,
    connection: &Connection,
    client: &ClientInfo,
    tx: &mut Tx,
    rx: &mut Rx,
) -> Result<bool> {
    let frame = read_request(rx).await?;
    let joined = if frame.op() == Ok(PeekInvite::OP) {
        let reply = match frame.decode::<PeekInvite>() {
            Ok(PeekInvite { code }) => peek(app, &code).await,
            Err(e) => Err(e),
        };
        match reply {
            Ok(preview) => tx.ok(&preview).await?,
            Err(e) => tx.err(&e).await?,
        }
        false
    } else if frame.op() == Ok(JoinAccount::OP) {
        let reply = match frame.decode::<JoinAccount>() {
            Ok(JoinAccount { code }) => join(app, connection, client, &code).await,
            Err(e) => Err(e),
        };
        match reply {
            Ok(()) => {
                tx.ok(&()).await?;
                true
            }
            Err(e) => {
                tx.err(&e).await?;
                false
            }
        }
    } else {
        tx.err(&ApiError::Unauthorized {}).await?;
        false
    };
    tx.finish()?;
    Ok(joined)
}

async fn peek(app: &App, code: &str) -> Result<JoinPreview, ApiError> {
    let code = link::join_code(code).ok_or_else(|| ApiError::bad_request("not a join code"))?;
    let inviter = app
        .store
        .peek_invite(&code)
        .await
        .map_err(internal)?
        .ok_or(ApiError::NotFound {})?;
    Ok(JoinPreview {
        name: inviter.name,
        kind: inviter.kind,
    })
}

async fn join(
    app: &App,
    connection: &Connection,
    client: &ClientInfo,
    code: &str,
) -> Result<(), ApiError> {
    let code = link::join_code(code).ok_or_else(|| ApiError::bad_request("not a join code"))?;
    let (name, kind) = match client.platform {
        Some(Platform::Macos {}) => ("Mac", DeviceKind::Desktop {}),
        Some(Platform::Ios {}) => ("iPhone", DeviceKind::Phone {}),
        Some(Platform::Android {}) | None => ("Phone", DeviceKind::Phone {}),
    };
    let key = NewKey {
        endpoint: connection.remote_id(),
        name,
        kind,
    };
    let target = app
        .store
        .redeem_invite(&code, key)
        .await
        .map_err(internal)?
        .ok_or(ApiError::NotFound {})?;
    log::info!(
        "a new key joined the owner; user={target} key={}",
        connection.remote_id()
    );
    if let Some(user) = app.user(&target) {
        user.changed("");
    }
    Ok(())
}

fn internal(e: anyhow::Error) -> ApiError {
    log::warn!("guest call failed: {e:#}");
    ApiError::Internal {}
}
