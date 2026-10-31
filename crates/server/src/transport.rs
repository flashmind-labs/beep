//! Serves the Beep protocol (see `beep_proto::wire`) on iroh connections.

use crate::{
    api,
    app::{Admission, App},
    client::CLOSE_NOT_INVITED,
    guest,
    protocol::{
        ClientInfo, DeviceKind, Hello, Platform, Rpc,
        wire::{Frame, Rx, Tx, framed},
    },
    push::{CLOSE_REVOKED, Pusher},
    router::{Cx, Router, read_request},
};
use anyhow::Result;
use iroh::{
    endpoint::{Connection, RecvStream, SendStream, VarInt},
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
        let signed_in = match self.app.admit(&connection.remote_id()).await {
            Ok(Admission::SignedIn(signed_in)) => signed_in,
            Ok(Admission::Revoked) => {
                connection.close(VarInt::from_u32(CLOSE_REVOKED), b"removed");
                return Ok(());
            }
            Ok(Admission::NotInvited) => {
                connection.close(VarInt::from_u32(CLOSE_NOT_INVITED), b"not invited");
                return Ok(());
            }
            Ok(Admission::Guest) => {
                let Ok((send, recv)) = connection.accept_bi().await else {
                    return Ok(());
                };
                let client = match open(send, recv).await {
                    Ok(Opening::Hello(client)) => client,
                    // An app from before `Hello` predates joining, too.
                    Ok(Opening::Request { .. }) => ClientInfo::LEGACY,
                    Err(e) => {
                        log::debug!("guest connection opened badly: {e:#}");
                        return Ok(());
                    }
                };
                guest::serve(self.app.clone(), connection, client).await;
                return Ok(());
            }
            Err(e) => {
                log::warn!("sign in failed: {e:#}");
                connection.close(VarInt::from_u32(CLOSE_UNAVAILABLE), b"unavailable");
                return Ok(());
            }
        };
        self.pusher.connected(connection.clone());
        // The first stream is the client's `Hello`, or a request from an app
        // that predates it.
        let Ok((send, recv)) = connection.accept_bi().await else {
            return Ok(());
        };
        let opening = match open(send, recv).await {
            Ok(opening) => opening,
            Err(e) => {
                log::debug!("connection opened badly: {e:#}");
                return Ok(());
            }
        };
        let (client, first) = match opening {
            Opening::Hello(client) => (client, None),
            Opening::Request { frame, tx, rx } => (ClientInfo::LEGACY, Some((frame, tx, *rx))),
        };
        log::debug!(
            "client connected; user={} wire={} platform={:?} version={:?}",
            signed_in.user.id(),
            client.wire,
            client.platform,
            client.version
        );
        // A key makes its account as a phone; one the Mac app made it with
        // is a computer.
        let mut device = signed_in.device;
        if signed_in.primary
            && matches!(device, DeviceKind::Phone {})
            && matches!(client.platform, Some(Platform::Macos {}))
        {
            device = DeviceKind::Desktop {};
            let endpoint = connection.remote_id();
            if let Err(e) = self.app.store.set_key_kind(&endpoint, device).await {
                log::warn!("unable to mark a key as a computer: {e:#}");
            }
        }
        let cx = Cx {
            app: self.app.clone(),
            user: signed_in.user,
            device,
            primary: signed_in.primary,
            connection: connection.clone(),
            pusher: self.pusher.clone(),
            client: Arc::new(client),
        };
        let streams = Arc::new(Semaphore::new(MAX_STREAMS));
        if let Some((frame, tx, rx)) = first {
            let Ok(permit) = streams.clone().acquire_owned().await else {
                return Ok(());
            };
            let router = self.router.clone();
            let cx = cx.clone();
            tokio::spawn(async move {
                let _permit = permit;
                if let Err(e) = router.dispatch(cx, frame, tx, rx).await {
                    log::debug!("stream ended: {e:#}");
                }
            });
        }
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

/// How a connection's first stream opened.
enum Opening {
    Hello(ClientInfo),
    /// An app from before `Hello`, starting with a request.
    Request {
        frame: Frame,
        tx: Tx,
        rx: Box<Rx>,
    },
}

/// Reads the first stream, answering a `Hello` on it.
async fn open(send: SendStream, recv: RecvStream) -> Result<Opening> {
    let (mut tx, mut rx) = framed(send, recv);
    let frame = read_request(&mut rx).await?;
    if frame.tag != Hello::OP as u8 {
        return Ok(Opening::Request {
            frame,
            tx,
            rx: Box::new(rx),
        });
    }
    match frame.decode::<Hello>() {
        Ok(hello) => {
            tx.ok(&()).await?;
            tx.finish()?;
            Ok(Opening::Hello(hello.into()))
        }
        Err(e) => {
            tx.err(&e).await?;
            tx.finish()?;
            Ok(Opening::Hello(ClientInfo::LEGACY))
        }
    }
}
