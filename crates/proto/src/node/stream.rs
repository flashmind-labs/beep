//! Msgpack messages on one stream of the node link.

use super::control::NODE_ALPN_NAMED;
use crate::lenient;
use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt};
use iroh::endpoint::{Connection, RecvStream, SendStream};
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

/// How a `LinkTx` writes a struct.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// An array of its fields, read by position.
    Compact,
    /// A map with the field names, which only the link to a server of the
    /// `beep-node/1` kind needs: released nodes and servers read maps, and
    /// skip a field they do not know only when it is named.
    Named,
}

/// Sends messages on one stream of the node link.
pub struct LinkTx {
    framed: FramedWrite<Out, LengthDelimitedCodec>,
    encoding: Encoding,
}

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
            LinkTx::new(Out::Local(to_node), Encoding::Compact),
            LinkRx(FramedRead::new(In::Local(from_node), codec())),
        ),
        node: (
            LinkTx::new(Out::Local(node_writes), Encoding::Compact),
            LinkRx(FramedRead::new(In::Local(node_reads), codec())),
        ),
    }
}

/// A stream whose messages are compact, as every stream of the mesh is.
pub fn link(send: SendStream, recv: RecvStream) -> (LinkTx, LinkRx) {
    link_encoded(send, recv, Encoding::Compact)
}

/// A stream of `connection`, written the way its protocol is: maps on
/// `beep-node/1`, compact otherwise.
pub fn link_on(connection: &Connection, send: SendStream, recv: RecvStream) -> (LinkTx, LinkRx) {
    let encoding = match connection.alpn() {
        NODE_ALPN_NAMED => Encoding::Named,
        _ => Encoding::Compact,
    };
    link_encoded(send, recv, encoding)
}

fn link_encoded(send: SendStream, recv: RecvStream, encoding: Encoding) -> (LinkTx, LinkRx) {
    (
        LinkTx::new(Out::Net(send), encoding),
        LinkRx(FramedRead::new(In::Net(recv), codec())),
    )
}

impl LinkTx {
    fn new(out: Out, encoding: Encoding) -> Self {
        Self {
            framed: FramedWrite::new(out, codec()),
            encoding,
        }
    }

    pub async fn send<T: Serialize>(&mut self, message: &T) -> Result<()> {
        let body = match self.encoding {
            Encoding::Compact => rmp_serde::to_vec(message)?,
            Encoding::Named => rmp_serde::to_vec_named(message)?,
        };
        self.framed.send(body.into()).await?;
        Ok(())
    }

    /// Finishes the stream. On the network it waits until the peer read all
    /// of it or stopped reading; on an in-process link it returns at once.
    pub async fn finish(self) -> Result<()> {
        match self.framed.into_inner() {
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
        let message = lenient::from_slice(&frame).context("could not read a node message")?;
        Ok(Some(message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{Down, NodeHello};
    use serde::Deserialize;

    #[derive(Serialize)]
    struct NewerHello {
        version: String,
        name: String,
        offers: Vec<String>,
        workspace: Option<String>,
        setup: Option<String>,
        credentials: bool,
        checkouts: bool,
        agent_settings: bool,
        branch_checkouts: bool,
        added_later: u32,
    }

    #[derive(Debug, PartialEq, Deserialize)]
    struct Pair {
        a: u8,
    }

    #[derive(Serialize)]
    struct NewerPair {
        a: u8,
        b: u8,
    }

    #[tokio::test]
    async fn a_node_message_with_a_field_from_a_newer_build_is_read() {
        let LocalLink {
            caller: (mut tx, _),
            node: (_, mut rx),
        } = local_link();
        tx.send(&NewerHello {
            version: "9.0.0".into(),
            name: "box".into(),
            offers: vec!["shell/1".into()],
            workspace: None,
            setup: None,
            credentials: true,
            checkouts: true,
            agent_settings: true,
            branch_checkouts: true,
            added_later: 5,
        })
        .await
        .expect("sends");
        let hello: NodeHello = rx.recv().await.expect("reads").expect("a message");
        assert_eq!(hello.name, "box");
        assert!(hello.branch_checkouts);
        tx.send(&NewerPair { a: 1, b: 2 }).await.expect("sends");
        assert_eq!(rx.recv::<Pair>().await.expect("reads"), Some(Pair { a: 1 }));
    }

    #[tokio::test]
    async fn a_link_writes_compact_or_named_as_it_was_built() {
        let (to_node, mut raw) = duplex(LOCAL_BUFFER);
        let mut tx = LinkTx::new(Out::Local(to_node), Encoding::Named);
        tx.send(&Down::Welcome { tools: true })
            .await
            .expect("sends");
        let mut named = vec![0u8; 64];
        let n = tokio::io::AsyncReadExt::read(&mut raw, &mut named)
            .await
            .expect("reads");
        assert!(named[..n].windows(5).any(|w| w == b"tools"));
        let (to_node, mut raw) = duplex(LOCAL_BUFFER);
        let mut tx = LinkTx::new(Out::Local(to_node), Encoding::Compact);
        tx.send(&Down::Welcome { tools: true })
            .await
            .expect("sends");
        let mut compact = vec![0u8; 64];
        let n = tokio::io::AsyncReadExt::read(&mut raw, &mut compact)
            .await
            .expect("reads");
        assert!(!compact[..n].windows(5).any(|w| w == b"tools"));
    }
}
