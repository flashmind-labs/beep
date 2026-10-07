//! Serves the Beep protocol (see `beep_proto::wire`) on iroh connections.

use crate::{
    api,
    app::App,
    push::{CLOSE_REVOKED, Pusher},
    router::{Cx, Router},
};
use iroh::{
    endpoint::{Connection, VarInt},
    protocol::{AcceptError, ProtocolHandler},
};
use std::{fmt, sync::Arc, time::Duration};
use tokio::sync::Semaphore;

pub use crate::protocol::wire::ALPN;

const MAX_STREAMS: usize = 32;
pub(crate) const READ_TIMEOUT: Duration = Duration::from_secs(10);
const CLOSE_UNAVAILABLE: u32 = 503;

#[derive(Clone)]
pub struct BeepHandler {
    app: Arc<App>,
    pusher: Arc<Pusher>,
    router: Arc<Router>,
}

impl fmt::Debug for BeepHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BeepHandler").finish()
    }
}

impl BeepHandler {
    pub fn new(app: Arc<App>, pusher: Arc<Pusher>) -> Self {
        Self {
            app,
            pusher,
            router: Arc::new(api::router()),
        }
    }
}

impl ProtocolHandler for BeepHandler {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        // The key the client connected with is who it is.
        let signed_in = match self.app.sign_in(&connection.remote_id()).await {
            Ok(Some(signed_in)) => signed_in,
            Ok(None) => {
                connection.close(VarInt::from_u32(CLOSE_REVOKED), b"removed");
                return Ok(());
            }
            Err(e) => {
                log::warn!("sign in failed: {e:#}");
                connection.close(VarInt::from_u32(CLOSE_UNAVAILABLE), b"unavailable");
                return Ok(());
            }
        };
        self.pusher.connected(connection.clone());
        let cx = Cx {
            app: self.app.clone(),
            user: signed_in.user,
            device: signed_in.device,
            connection: connection.clone(),
            pusher: self.pusher.clone(),
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
