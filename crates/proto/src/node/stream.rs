//! Msgpack messages on one stream of the node link.

use crate::lenient;
use anyhow::{Context, Result, bail, ensure};
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use iroh::endpoint::{RecvStream, SendStream};
use serde::{Serialize, de::DeserializeOwned};
use std::io::{self, Read};
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, DuplexStream, ReadBuf, duplex};
use tokio_io_timeout::{TimeoutReader, TimeoutWriter};
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};

/// The largest message either side reads.
pub const MAX_FRAME: usize = 8 * 1024 * 1024;
/// Frames at or above this size are compressed with zstd when negotiated.
pub const ZSTD_THRESHOLD: usize = 1024;
/// Zstd compression level when negotiated on a stream.
pub const ZSTD_LEVEL: i32 = 3;
const FRAME_RAW: u8 = 0;
const FRAME_ZSTD: u8 = 1;

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
pub struct LinkTx {
    framed: FramedWrite<Pin<Box<TimeoutWriter<Out>>>, LengthDelimitedCodec>,
    zstd: bool,
}

/// Reads messages from one stream of the node link.
pub struct LinkRx {
    framed: FramedRead<Pin<Box<TimeoutReader<In>>>, LengthDelimitedCodec>,
    zstd: bool,
    max_frame: usize,
}

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
            LinkTx::new(Out::Local(to_node)),
            LinkRx::new(In::Local(from_node)),
        ),
        node: (
            LinkTx::new(Out::Local(node_writes)),
            LinkRx::new(In::Local(node_reads)),
        ),
    }
}

/// A stream whose messages are compact, as every stream of the mesh is.
pub fn link(send: SendStream, recv: RecvStream) -> (LinkTx, LinkRx) {
    (LinkTx::new(Out::Net(send)), LinkRx::new(In::Net(recv)))
}

impl LinkTx {
    fn new(out: Out) -> Self {
        Self {
            framed: FramedWrite::new(Box::pin(TimeoutWriter::new(out)), codec()),
            zstd: false,
        }
    }

    /// Enables negotiated zstd frame compression for subsequent messages.
    pub fn enable_zstd(&mut self) {
        self.zstd = true;
    }

    /// Whether zstd frame compression is active on this sender.
    pub fn is_zstd(&self) -> bool {
        self.zstd
    }

    /// Limits inactivity between writes, without limiting the whole message.
    pub fn set_idle_timeout(&mut self, idle: Duration) {
        self.framed
            .get_mut()
            .as_mut()
            .set_timeout_pinned(Some(idle));
    }

    pub async fn send<T: Serialize>(&mut self, message: &T) -> Result<()> {
        if !self.zstd {
            let body = rmp_serde::to_vec(message)?;
            self.framed.send(Into::<Bytes>::into(body)).await?;
            return Ok(());
        }

        let mut raw = Vec::with_capacity(64);
        raw.push(FRAME_RAW);
        rmp_serde::encode::write(&mut raw, message)?;
        let body = &raw[1..];
        let payload = if body.len() >= ZSTD_THRESHOLD {
            match zstd::bulk::compress(body, ZSTD_LEVEL) {
                Ok(compressed) if compressed.len() < body.len() => {
                    let mut out = Vec::with_capacity(1 + compressed.len());
                    out.push(FRAME_ZSTD);
                    out.extend_from_slice(&compressed);
                    out
                }
                _ => raw,
            }
        } else {
            raw
        };
        self.framed.send(Into::<Bytes>::into(payload)).await?;
        Ok(())
    }

    /// Finishes the stream. On the network it waits until the peer read all
    /// of it or stopped reading; on an in-process link it returns at once.
    pub async fn finish(mut self) -> Result<()> {
        match self.framed.get_mut().as_mut().get_pin_mut().get_mut() {
            Out::Net(send) => {
                send.finish()?;
                send.stopped().await?;
            }
            Out::Local(pipe) => pipe.shutdown().await?,
        }

        Ok(())
    }
}

impl LinkRx {
    fn new(input: In) -> Self {
        Self {
            framed: FramedRead::new(Box::pin(TimeoutReader::new(input)), codec()),
            zstd: false,
            max_frame: MAX_FRAME,
        }
    }

    /// Enables negotiated zstd frame decompression for subsequent messages.
    pub fn enable_zstd(&mut self) {
        self.zstd = true;
    }

    /// Whether zstd frame decompression is active on this receiver.
    pub fn is_zstd(&self) -> bool {
        self.zstd
    }

    /// Limits inactivity between reads, including within a message frame.
    pub fn set_idle_timeout(&mut self, idle: Duration) {
        self.framed
            .get_mut()
            .as_mut()
            .set_timeout_pinned(Some(idle));
    }

    /// Limits the frames read from here on to `max` bytes, at most
    /// `MAX_FRAME`, such as for a stranger's first message.
    pub fn set_max_frame(&mut self, max: usize) {
        self.max_frame = max.min(MAX_FRAME);
        self.framed
            .decoder_mut()
            .set_max_frame_length(self.max_frame);
    }

    /// The next message, `None` once the peer finished the stream.
    pub async fn recv<T: DeserializeOwned>(&mut self) -> Result<Option<T>> {
        self.recv_with(lenient::from_slice).await
    }

    /// The next message as `read` takes it from the frame's bytes, `None`
    /// once the peer finished the stream.
    pub async fn recv_with<T, E>(
        &mut self,
        read: impl FnOnce(&[u8]) -> Result<T, E>,
    ) -> Result<Option<T>>
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        let Some(frame) = self.framed.next().await else {
            return Ok(None);
        };
        let frame = frame?;

        if !self.zstd {
            let message = read(&frame).context("could not read a node message")?;
            return Ok(Some(message));
        }

        let (&flag, payload) = frame.split_first().context("empty compressed node frame")?;

        match flag {
            FRAME_RAW => {
                let message = read(payload).context("could not read a node message")?;
                Ok(Some(message))
            }
            FRAME_ZSTD => {
                let limit = self.max_frame;
                let mut decoder = zstd::stream::read::Decoder::new(payload)
                    .context("could not start zstd decoder for node message")?;
                let mut decoded = Vec::new();
                decoder
                    .by_ref()
                    .take((limit + 1) as u64)
                    .read_to_end(&mut decoded)
                    .context("could not decompress a node message")?;
                ensure!(
                    decoded.len() <= limit,
                    "decompressed node message exceeds frame limit"
                );
                let message = read(&decoded).context("could not read a node message")?;
                Ok(Some(message))
            }
            other => bail!("unknown node frame compression flag {other}"),
        }
    }
}

#[cfg(test)]
#[path = "../tests/node_stream.rs"]
mod tests;
