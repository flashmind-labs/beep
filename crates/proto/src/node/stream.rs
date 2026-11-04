//! Msgpack messages on one stream of the node link.

use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt};
use iroh::endpoint::{RecvStream, SendStream};
use serde::{Serialize, de::DeserializeOwned};
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};

/// The largest message either side reads.
pub const MAX_FRAME: usize = 8 * 1024 * 1024;

/// Sends messages on one stream of the node link.
pub struct LinkTx(FramedWrite<SendStream, LengthDelimitedCodec>);

/// Reads messages from one stream of the node link.
pub struct LinkRx(FramedRead<RecvStream, LengthDelimitedCodec>);

pub fn link(send: SendStream, recv: RecvStream) -> (LinkTx, LinkRx) {
    let codec = || {
        LengthDelimitedCodec::builder()
            .max_frame_length(MAX_FRAME)
            .new_codec()
    };
    (
        LinkTx(FramedWrite::new(send, codec())),
        LinkRx(FramedRead::new(recv, codec())),
    )
}

impl LinkTx {
    pub async fn send<T: Serialize>(&mut self, message: &T) -> Result<()> {
        let body = rmp_serde::to_vec_named(message)?;
        self.0.send(body.into()).await?;
        Ok(())
    }

    /// Finishes the stream and waits until the peer read all of it or
    /// stopped reading.
    pub async fn finish(self) -> Result<()> {
        let mut send = self.0.into_inner();
        send.finish()?;
        send.stopped().await?;
        Ok(())
    }
}

impl LinkRx {
    /// Reads no message longer than `max` bytes from now on, such as a
    /// hello from a key nobody linked yet; `MAX_FRAME` is the default.
    pub fn set_max_frame(&mut self, max: usize) {
        self.0.decoder_mut().set_max_frame_length(max);
    }

    /// The next message, `None` once the peer finished the stream.
    pub async fn recv<T: DeserializeOwned>(&mut self) -> Result<Option<T>> {
        let Some(frame) = self.0.next().await else {
            return Ok(None);
        };
        let frame = frame?;
        let message = rmp_serde::from_slice(&frame).context("could not read a node message")?;
        Ok(Some(message))
    }
}
