//! Framing over iroh bi streams.
//!
//! Frames are length-delimited: one tag byte, then a msgpack body. A request
//! frame's tag is its `Op`; a reply frame's tag says whether the body is the
//! reply or an `ApiError`.
//!
//! The first bi stream on a connection carries `Auth`. After that each request
//! gets its own bi stream: one request frame in, reply frames out until the
//! server finishes the stream.

use crate::{ApiError, Rpc};
use anyhow::{Context, Result, bail};
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use iroh::endpoint::{RecvStream, SendStream};
use serde::{Serialize, de::DeserializeOwned};
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};

pub const ALPN: &[u8] = b"beep/1";

const MAX_FRAME: usize = 8 * 1024 * 1024;

/// Reply frame tags.
const OK: u8 = 0;
const ERR: u8 = 1;

fn codec() -> LengthDelimitedCodec {
    LengthDelimitedCodec::builder()
        .max_frame_length(MAX_FRAME)
        .new_codec()
}

pub struct Tx(FramedWrite<SendStream, LengthDelimitedCodec>);
pub struct Rx(FramedRead<RecvStream, LengthDelimitedCodec>);

pub fn framed(send: SendStream, recv: RecvStream) -> (Tx, Rx) {
    (
        Tx(FramedWrite::new(send, codec())),
        Rx(FramedRead::new(recv, codec())),
    )
}

/// One received frame, body not yet decoded.
pub struct Frame {
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
