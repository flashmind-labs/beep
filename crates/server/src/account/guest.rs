//! A key nobody linked: it may only join an account with a code, or sign in on a hosted server.

use crate::{
    account::login,
    app::App,
    protocol::{
        ApiError, CompleteLogin, DeviceKind, JoinAccount, JoinPreview, Op, PeekInvite, Platform,
        StartLogin, http, link,
    },
    store::NewKey,
};
use iroh::EndpointId;
use serde::{Serialize, de::DeserializeOwned};

/// Answers a guest's request `op`; any other than joining or signing in is refused.
pub(crate) async fn call(
    app: &App,
    key: EndpointId,
    platform: Option<Platform>,
    op: Op,
    body: &[u8],
) -> Result<Vec<u8>, ApiError> {
    match op {
        Op::PeekInvite => {
            let PeekInvite { code } = decode(body)?;
            encode(&peek(app, &code).await?)
        }
        Op::StartLogin => {
            let StartLogin { device } = decode(body)?;
            encode(&login::start(app, &key, &device).await?)
        }
        Op::CompleteLogin => {
            let CompleteLogin { token } = decode(body)?;
            login::complete(app, new_key(key, platform), token.expose()).await?;
            encode(&())
        }
        Op::JoinAccount => {
            let JoinAccount { code } = decode(body)?;
            join(app, key, platform, &code).await?;
            encode(&())
        }
        _ => Err(ApiError::Unauthorized {}),
    }
}

fn decode<R: DeserializeOwned>(body: &[u8]) -> Result<R, ApiError> {
    http::decode(body).map_err(|e| ApiError::bad_request(e.to_string()))
}

fn encode<T: Serialize>(reply: &T) -> Result<Vec<u8>, ApiError> {
    http::encode(reply).map_err(|_| ApiError::Internal {})
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
    key: EndpointId,
    platform: Option<Platform>,
    code: &str,
) -> Result<(), ApiError> {
    let code = link::join_code(code).ok_or_else(|| ApiError::bad_request("not a join code"))?;
    let target = app
        .store
        .redeem_invite(&code, new_key(key, platform))
        .await
        .map_err(internal)?
        .ok_or(ApiError::NotFound {})?;
    log::info!("a new key joined the owner; user={target} key={key}");
    if let Some(user) = app.user(&target) {
        user.changed("");
    }
    Ok(())
}

/// `key` as a new device of an account, named by its app.
pub(crate) fn new_key(key: EndpointId, platform: Option<Platform>) -> NewKey<'static> {
    let (name, kind) = match platform {
        Some(Platform::Macos {}) => ("Mac", DeviceKind::Desktop {}),
        Some(Platform::Ios {}) => ("iPhone", DeviceKind::Phone {}),
        Some(Platform::Android {}) | None => ("Phone", DeviceKind::Phone {}),
    };
    NewKey {
        endpoint: key,
        name,
        kind,
    }
}

fn internal(e: anyhow::Error) -> ApiError {
    log::warn!("guest call failed: {e:#}");
    ApiError::Internal {}
}
