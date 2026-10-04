//! Serves the Beep protocol (see `beep_proto::wire`) on iroh connections.

use crate::{
    api,
    app::App,
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
        // The key the client connected with is who it is.
        let user = match self.app.sign_in(&connection.remote_id()).await {
            Ok(user) => user,
            Err(e) => {
                log::warn!("sign in failed: {e:#}");
                connection.close(VarInt::from_u32(CLOSE_UNAVAILABLE), b"unavailable");
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
