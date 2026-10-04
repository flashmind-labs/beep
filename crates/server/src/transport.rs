//! Serves the Beep protocol (see `beep_proto::wire`) on iroh connections.

use crate::{
    api,
    app::App,
    protocol::{
        ApiError, Auth, Op,
        wire::{Rx, framed},
    },
    router::{Cx, Router},
    user::User,
};
use anyhow::{Context, Result, ensure};
use iroh::{
    endpoint::{Connection, VarInt},
    protocol::{AcceptError, ProtocolHandler},
};
use std::{fmt, sync::Arc, time::Duration};
use tokio::{sync::Semaphore, time::timeout};

pub use crate::protocol::wire::ALPN;

const MAX_STREAMS: usize = 32;
pub(crate) const READ_TIMEOUT: Duration = Duration::from_secs(10);
const CLOSE_UNAUTHORIZED: u32 = 401;

#[derive(Clone)]
pub struct BeepHandler {
    app: Arc<App>,
    router: Arc<Router>,
}

impl fmt::Debug for BeepHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BeepHandler").finish()
    }
}

impl BeepHandler {
    pub fn new(app: Arc<App>) -> Self {
        Self {
            app,
            router: Arc::new(api::router()),
        }
    }
}

impl ProtocolHandler for BeepHandler {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        let user = match authenticate(&self.app, &connection).await {
            Ok(user) => user.clone(),
            Err(e) => {
                log::debug!("connection rejected: {e:#}");
                connection.close(VarInt::from_u32(CLOSE_UNAUTHORIZED), b"unauthorized");
                return Ok(());
            }
        };
        let cx = Cx {
            app: self.app.clone(),
            user,
            connection: connection.clone(),
        };
        let streams = Arc::new(Semaphore::new(MAX_STREAMS));
        while let Ok((send, recv)) = connection.accept_bi().await {
            let Ok(permit) = streams.clone().acquire_owned().await else {
                break;
            };
            let router = self.router.clone();
            let cx = cx.clone();
            tokio::spawn(async move {
                let _permit = permit;
                if let Err(e) = router.serve(cx, send, recv).await {
                    log::debug!("stream ended: {e:#}");
                }
            });
        }
        Ok(())
    }
}

/// Reads `Auth` from the first stream and returns the user. A bad token is
/// answered with `Unauthorized` before the caller closes the connection.
async fn authenticate<'a>(app: &'a App, connection: &Connection) -> Result<&'a Arc<User>> {
    let (send, recv) = timeout(READ_TIMEOUT, connection.accept_bi())
        .await
        .context("auth timed out")??;
    let (mut tx, mut rx) = framed(send, recv);
    let token = read_auth(&mut rx).await?;
    let user = app
        .auth
        .authenticate(Some(&format!("Bearer {token}")))
        .and_then(|id| app.user(&id));
    let Some(user) = user else {
        tx.err(&ApiError::Unauthorized {}).await?;
        tx.finish()?;
        // Give the client time to read the reply before the connection closes.
        let _ = timeout(READ_TIMEOUT, connection.closed()).await;
        anyhow::bail!("invalid token");
    };
    tx.ok(&()).await?;
    tx.finish()?;
    Ok(user)
}

async fn read_auth(rx: &mut Rx) -> Result<String> {
    let frame = timeout(READ_TIMEOUT, rx.frame())
        .await
        .context("auth timed out")??
        .context("auth stream empty")?;
    ensure!(frame.tag == Op::Auth as u8, "first request must be Auth");
    let Auth { token } = frame.decode()?;
    Ok(token)
}
