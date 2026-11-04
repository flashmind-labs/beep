//! Msgpack messages on one stream of the node link.

use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt};
use iroh::endpoint::{RecvStream, SendStream};
use serde::{Serialize, de::DeserializeOwned};
use std::io;
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, DuplexStream, ReadBuf, duplex};
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};

/// The largest message either side reads.
pub const MAX_FRAME: usize = 8 * 1024 * 1024;

/// What each direction of an in-process link can hold before the writer waits.
const LOCAL_BUFFER: usize = 64 * 1024;

/// The sending half of a stream: on the network, or in memory.
enum Out {
    Net(SendStream),
    Local(DuplexStream),
}

/// The receiving half of a stream: on the network, or in memory.
enum In {
    Net(RecvStream),
    Local(DuplexStream),
}

impl AsyncWrite for Out {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Out::Net(s) => AsyncWrite::poll_write(Pin::new(s), cx, buf),
            Out::Local(s) => AsyncWrite::poll_write(Pin::new(s), cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Out::Net(s) => AsyncWrite::poll_flush(Pin::new(s), cx),
            Out::Local(s) => AsyncWrite::poll_flush(Pin::new(s), cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Out::Net(s) => AsyncWrite::poll_shutdown(Pin::new(s), cx),
            Out::Local(s) => AsyncWrite::poll_shutdown(Pin::new(s), cx),
        }
    }
}

impl AsyncRead for In {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            In::Net(s) => AsyncRead::poll_read(Pin::new(s), cx, buf),
            In::Local(s) => AsyncRead::poll_read(Pin::new(s), cx, buf),
        }
    }
}

/// Sends messages on one stream of the node link.
pub struct LinkTx(FramedWrite<Out, LengthDelimitedCodec>);

/// Reads messages from one stream of the node link.
pub struct LinkRx(FramedRead<In, LengthDelimitedCodec>);

/// Both ends of an in-memory stream pair: what the caller holds and what
/// the node's session gets.
pub struct LocalLink {
    pub caller: (LinkTx, LinkRx),
    pub node: (LinkTx, LinkRx),
}

fn codec() -> LengthDelimitedCodec {
    LengthDelimitedCodec::builder()
        .max_frame_length(MAX_FRAME)
        .new_codec()
}

/// A session stream with no network under it, for a desktop that talks to
/// its own node in the same process.
pub fn local_link() -> LocalLink {
    let (to_node, node_reads) = duplex(LOCAL_BUFFER);
    let (node_writes, from_node) = duplex(LOCAL_BUFFER);
    LocalLink {
        caller: (
            LinkTx(FramedWrite::new(Out::Local(to_node), codec())),
            LinkRx(FramedRead::new(In::Local(from_node), codec())),
        ),
        node: (
            LinkTx(FramedWrite::new(Out::Local(node_writes), codec())),
            LinkRx(FramedRead::new(In::Local(node_reads), codec())),
        ),
    }
}

pub fn link(send: SendStream, recv: RecvStream) -> (LinkTx, LinkRx) {
    (
        LinkTx(FramedWrite::new(Out::Net(send), codec())),
        LinkRx(FramedRead::new(In::Net(recv), codec())),
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
        match self.0.into_inner() {
            Out::Net(mut send) => {
                send.finish()?;
                send.stopped().await?;
            }
            Out::Local(mut pipe) => pipe.shutdown().await?,
        }
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
