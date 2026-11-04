//! A key nobody linked: it may only join an account with a code, or sign in on a hosted server.

use crate::{
    account::login,
    api::router::read_request,
    app::App,
    protocol::{
        ApiError, ClientInfo, CompleteLogin, DeviceKind, JoinAccount, JoinPreview, PeekInvite,
        Platform, Rpc, StartLogin, link,
        wire::{Rx, Tx, framed},
    },
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

/// Answers one call; true once the key joined an account.
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
    } else if frame.op() == Ok(StartLogin::OP) {
        let reply = match frame.decode::<StartLogin>() {
            Ok(StartLogin { device }) => login::start(app, &connection.remote_id(), &device).await,
            Err(e) => Err(e),
        };
        match reply {
            Ok(url) => tx.ok(&url).await?,
            Err(e) => tx.err(&e).await?,
        }
        false
    } else if frame.op() == Ok(CompleteLogin::OP) {
        let reply = match frame.decode::<CompleteLogin>() {
            Ok(CompleteLogin { token }) => {
                login::complete(app, new_key(connection, client), token.expose()).await
            }
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
    let target = app
        .store
        .redeem_invite(&code, new_key(connection, client))
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

/// This connection's key as a new device of an account, named by its app.
pub(crate) fn new_key(connection: &Connection, client: &ClientInfo) -> NewKey<'static> {
    let (name, kind) = match client.platform {
        Some(Platform::Macos {}) => ("Mac", DeviceKind::Desktop {}),
        Some(Platform::Ios {}) => ("iPhone", DeviceKind::Phone {}),
        Some(Platform::Android {}) | None => ("Phone", DeviceKind::Phone {}),
    };
    NewKey {
        endpoint: connection.remote_id(),
        name,
        kind,
    }
}

fn internal(e: anyhow::Error) -> ApiError {
    log::warn!("guest call failed: {e:#}");
    ApiError::Internal {}
}
