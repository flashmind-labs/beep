//! Framing over iroh bi streams.
//!
//! Frames are length-delimited: one tag byte, then a msgpack body. A request
//! frame's tag is its `Op`, or `WIDE` followed by the op as two bytes, big
//! endian, for an op past 254; a reply frame's tag says whether the body is
//! the reply or an `ApiError`.
//!
//! The client's iroh key is its identity, so there is no login. Each request
//! gets its own bi stream: one request frame in, reply frames out until the
//! server finishes the stream.

use crate::{ApiError, Op, Rpc};
use anyhow::{Context, Result, bail};
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use iroh::endpoint::{RecvStream, SendStream};
use serde::{Serialize, de::DeserializeOwned};
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};

/// `beep/1` clients sent a token first; `beep/2` ones never do.
pub const ALPN: &[u8] = b"beep/2";

/// Connections the server dials to the phone. They only carry notices: the
/// server opens a bi stream, sends one `Notice` frame and waits for an empty
/// reply as the ack. The same exchange runs on `ALPN` connections too.
pub const PUSH_ALPN: &[u8] = b"beep-push/1";

const MAX_FRAME: usize = 8 * 1024 * 1024;

/// Reply frame tags.
const OK: u8 = 0;
const ERR: u8 = 1;

/// The request frame tag for an op that does not fit one byte: the op's
/// two bytes follow it. Servers from before it answer that the op is
/// unknown.
pub const WIDE: u8 = 255;

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
    /// The op of a request frame. An unknown one is the client's fault.
    pub fn op(&self) -> Result<Op, ApiError> {
        if self.tag != WIDE {
            return Op::try_from(u16::from(self.tag));
        }
        match self.body.get(..2) {
            Some(&[high, low]) => Op::try_from(u16::from_be_bytes([high, low])),
            _ => Err(ApiError::bad_request("a wide op without its number")),
        }
    }

    /// The body as a request the server got. A body that does not decode is
    /// the client's fault, so it becomes `BadRequest`.
    pub fn decode<T: DeserializeOwned>(&self) -> Result<T, ApiError> {
        let body = match self.tag {
            WIDE => self.body.get(2..).unwrap_or_default(),
            _ => &self.body[..],
        };
        rmp_serde::from_slice(body).map_err(|e| ApiError::bad_request(e.to_string()))
    }

    /// The body as a reply frame. A body that does not decode is a transport
    /// fault, not an `ApiError`, so callers do not mistake it for the server
    /// refusing the request.
    fn decode_reply<T: DeserializeOwned>(&self) -> Result<T> {
        rmp_serde::from_slice(&self.body).context("could not read the server's reply")
    }

    fn into_reply<T: DeserializeOwned>(self) -> Result<T> {
        match self.tag {
            OK => self.decode_reply(),
            ERR => Err(self.decode_reply::<ApiError>()?.into()),
            tag => bail!("unknown reply tag {tag}"),
        }
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
        self.0.send(request_frame(request)?.into()).await?;
        Ok(())
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

/// A request's frame: its tag, then its body.
fn request_frame<R: Rpc>(request: &R) -> Result<Vec<u8>> {
    let op = R::OP as u16;
    let mut buf = match u8::try_from(op) {
        Ok(tag) => vec![tag],
        Err(_) => {
            let [high, low] = op.to_be_bytes();
            vec![WIDE, high, low]
        }
    };
    rmp_serde::encode::write(&mut buf, request)?;
    Ok(buf)
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
        frame.into_reply().map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(tag: u8, body: &[u8]) -> Frame {
        Frame {
            tag,
            body: Bytes::copy_from_slice(body),
        }
    }

    #[test]
    fn a_reply_that_does_not_decode_is_no_api_error() {
        let error = frame(OK, &[0xc1]).into_reply::<u64>().unwrap_err();
        assert!(error.downcast_ref::<ApiError>().is_none(), "{error:#}");
        let error = frame(ERR, &[0xc1]).into_reply::<u64>().unwrap_err();
        assert!(error.downcast_ref::<ApiError>().is_none(), "{error:#}");
    }

    /// The frame `Rx::frame` reads from `bytes`.
    fn read(bytes: Vec<u8>) -> Frame {
        let (&tag, body) = bytes.split_first().unwrap();
        frame(tag, body)
    }

    #[test]
    fn a_wide_op_round_trips_and_a_narrow_one_keeps_its_byte() {
        let wide = crate::GetAgentImage {
            machine_id: "m".into(),
            session: "s".into(),
            key: "k".into(),
        };
        let bytes = request_frame(&wide).unwrap();
        assert_eq!(bytes[..3], [WIDE, 1, 4]);
        let got = read(bytes);
        assert_eq!(got.op(), Ok(Op::GetAgentImage));
        assert_eq!(got.decode::<crate::GetAgentImage>(), Ok(wide));
        let narrow = crate::Health {};
        let bytes = request_frame(&narrow).unwrap();
        assert_eq!(bytes[0], crate::Health::OP as u8);
        let got = read(bytes);
        assert_eq!(got.op(), Ok(Op::Health));
        assert_eq!(got.decode::<crate::Health>(), Ok(narrow));
        assert!(frame(WIDE, &[1]).op().is_err(), "a cut wide op");
    }

    #[test]
    fn an_error_frame_is_an_api_error() {
        let body = rmp_serde::to_vec(&ApiError::NotFound {}).unwrap();
        let error = frame(ERR, &body).into_reply::<u64>().unwrap_err();
        assert!(matches!(
            error.downcast_ref::<ApiError>(),
            Some(ApiError::NotFound {})
        ));
        let body = rmp_serde::to_vec(&7u64).unwrap();
        assert_eq!(frame(OK, &body).into_reply::<u64>().unwrap(), 7);
    }
}
