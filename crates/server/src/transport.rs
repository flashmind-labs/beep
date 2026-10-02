//! Beep wire protocol over iroh.
//!
//! Frames are length-delimited: one tag byte, then a msgpack body. A request
//! frame's tag is its `Op`; a reply frame's tag says whether the body is the
//! reply or an `ApiError`.
//!
//! The first bi stream on a connection carries `Auth`; the server replies or
//! closes the connection. After that each request gets its own bi stream: one
//! request frame in, reply frames out until the server finishes the stream.

use crate::{
    api,
    app::App,
    protocol::{ApiError, Auth, Op, Rpc},
    router::{Cx, Router},
};
use anyhow::{Context, Result, anyhow, bail, ensure};
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use iroh::{
    endpoint::{Connection, RecvStream, SendStream, VarInt},
    protocol::{AcceptError, ProtocolHandler},
};
use serde::{Serialize, de::DeserializeOwned};
use std::{fmt, sync::Arc, time::Duration};
use tokio::{sync::Semaphore, time::timeout};
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};

pub const ALPN: &[u8] = b"beep/1";

const MAX_FRAME: usize = 8 * 1024 * 1024;
const MAX_STREAMS: usize = 32;
pub(crate) const READ_TIMEOUT: Duration = Duration::from_secs(10);
const CLOSE_UNAUTHORIZED: u32 = 401;

/// Reply frame tags.
const OK: u8 = 0;
const ERR: u8 = 1;

fn codec() -> LengthDelimitedCodec {
    LengthDelimitedCodec::builder()
        .max_frame_length(MAX_FRAME)
        .new_codec()
}

pub(crate) struct Tx(FramedWrite<SendStream, LengthDelimitedCodec>);
pub(crate) struct Rx(FramedRead<RecvStream, LengthDelimitedCodec>);

pub(crate) fn framed(send: SendStream, recv: RecvStream) -> (Tx, Rx) {
    (
        Tx(FramedWrite::new(send, codec())),
        Rx(FramedRead::new(recv, codec())),
    )
}

/// One received frame, body not yet decoded.
pub(crate) struct Frame {
    pub tag: u8,
    body: Bytes,
}

impl Frame {
    pub fn decode<T: DeserializeOwned>(&self) -> Result<T, ApiError> {
        rmp_serde::from_slice(&self.body).map_err(|e| ApiError::bad_request(e.to_string()))
    }
}

impl Tx {
    async fn send<T: Serialize>(&mut self, tag: u8, body: &T) -> Result<()> {
        let mut buf = vec![tag];
        rmp_serde::encode::write(&mut buf, body)?;
        self.0.send(buf.into()).await?;
        Ok(())
    }

    pub async fn request<R: Rpc>(&mut self, request: &R) -> Result<()> {
        self.send(R::OP as u8, request).await
    }

    pub async fn ok<T: Serialize>(&mut self, reply: &T) -> Result<()> {
        self.send(OK, reply).await
    }

    pub async fn err(&mut self, error: &ApiError) -> Result<()> {
        self.send(ERR, error).await
    }

    pub fn finish(&mut self) -> Result<()> {
        self.0.get_mut().finish()?;
        Ok(())
    }
}

impl Rx {
    /// Next frame, `None` once the peer finished the stream.
    pub async fn frame(&mut self) -> Result<Option<Frame>> {
        let Some(frame) = self.0.next().await else {
            return Ok(None);
        };
        let body = frame?.freeze();
        let (&tag, _) = body.split_first().context("empty frame")?;
        Ok(Some(Frame {
            tag,
            body: body.slice(1..),
        }))
    }

    /// Next reply, `None` once the server finished the stream. An error
    /// frame becomes an `ApiError`, recoverable from `anyhow::Error` by downcast.
    pub async fn reply<T: DeserializeOwned>(&mut self) -> Result<Option<T>> {
        let Some(frame) = self.frame().await? else {
            return Ok(None);
        };
        match frame.tag {
            OK => Ok(Some(frame.decode()?)),
            ERR => Err(frame.decode::<ApiError>()?.into()),
            tag => bail!("unknown reply tag {tag}"),
        }
    }
}

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
            Ok(user) => user,
            Err(e) => {
                log::debug!("connection rejected: {e:#}");
                connection.close(VarInt::from_u32(CLOSE_UNAUTHORIZED), b"unauthorized");
                return Ok(());
            }
        };
        let cx = Cx {
            app: self.app.clone(),
            user: user.into(),
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

async fn authenticate(app: &App, connection: &Connection) -> Result<String> {
    let (send, recv) = timeout(READ_TIMEOUT, connection.accept_bi())
        .await
        .context("auth timed out")??;
    let (mut tx, mut rx) = framed(send, recv);
    let frame = timeout(READ_TIMEOUT, rx.frame())
        .await
        .context("auth timed out")??
        .context("auth stream empty")?;
    ensure!(frame.tag == Op::Auth as u8, "first request must be Auth");
    let Auth { token } = frame.decode()?;
    let user = app
        .auth
        .authenticate(Some(&format!("Bearer {token}")))
        .ok_or_else(|| anyhow!("invalid token"))?;
    tx.ok(&()).await?;
    tx.finish()?;
    Ok(user)
}
