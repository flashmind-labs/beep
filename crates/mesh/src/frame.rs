//! Length-delimited msgpack frames on a stream: a four byte big-endian
//! length, then the compact (positional) msgpack body. A peer's declared
//! length is checked before any buffer is allocated.

use crate::MeshError;
use beep_proto::lenient;
use bytes::{Bytes, BytesMut};
use futures::StreamExt;
use iroh::endpoint::{RecvStream, SendStream};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fmt::Display;
use std::io::ErrorKind;
use std::time::Duration;
use tokio::time::timeout;
use tokio_util::codec::{Encoder, FramedRead, LengthDelimitedCodec};

/// The largest frame either side accepts.
pub(crate) const MAX_FRAME: usize = 4 * 1024 * 1024;
/// The largest frame a stranger may send, such as a join request.
pub(crate) const MAX_REQUEST: usize = 4 * 1024;
/// A peer that takes this long to accept a frame is stalled.
const WRITE: Duration = Duration::from_secs(10);

pub(crate) fn net(e: impl Display) -> MeshError {
    MeshError::Net(e.to_string())
}

fn codec(max: usize) -> LengthDelimitedCodec {
    LengthDelimitedCodec::builder()
        .max_frame_length(max)
        .new_codec()
}

pub(crate) async fn send<T: Serialize>(
    stream: &mut SendStream,
    value: &T,
) -> Result<(), MeshError> {
    let body = rmp_serde::to_vec(value)?;
    let mut out = BytesMut::with_capacity(body.len() + 4);
    codec(MAX_FRAME)
        .encode(Bytes::from(body), &mut out)
        .map_err(|_| MeshError::TooLarge)?;
    timeout(WRITE, stream.write_all(&out))
        .await
        .map_err(|_| net("write timed out"))?
        .map_err(net)
}

/// Reads frames from one stream. It keeps what it read past a frame, so one
/// reader serves the stream for its whole life.
pub(crate) struct FrameReader(FramedRead<RecvStream, LengthDelimitedCodec>);

impl FrameReader {
    /// A reader that refuses frames longer than `max`.
    pub(crate) fn new(stream: RecvStream, max: usize) -> Self {
        Self(FramedRead::new(stream, codec(max)))
    }

    /// The next frame, or `None` when the peer finished the stream cleanly
    /// between frames.
    pub(crate) async fn recv<T: DeserializeOwned>(&mut self) -> Result<Option<T>, MeshError> {
        match self.0.next().await {
            None => Ok(None),
            Some(Ok(body)) => Ok(Some(lenient::from_slice(&body)?)),
            Some(Err(e)) if e.kind() == ErrorKind::InvalidData => Err(MeshError::TooLarge),
            Some(Err(e)) => Err(net(e)),
        }
    }
}
