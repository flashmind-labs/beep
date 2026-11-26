//! Serving one `terminal/1` stream: the caller's requests answered from the
//! node's terminals, and an attached terminal streamed to it.

use super::{
    pty::Live,
    terminals::{Terminals, unknown},
};
use crate::lock;
use anyhow::Result;
use beep_proto::node::{
    LinkRx, LinkTx,
    terminal::{MAX_INPUT, Reply, Request},
};
use beep_proto::watched;
use serde_bytes::ByteBuf;
use std::sync::Arc;

/// The far side of a `terminal/1` stream: the caller, over the link, or
/// the node, over the terminal host's socket.
pub(super) trait Peer {
    async fn send(&mut self, reply: &Reply) -> Result<()>;
    async fn recv(&mut self) -> Result<Option<Request>>;
}

pub(super) struct Link {
    pub(super) tx: LinkTx,
    pub(super) rx: LinkRx,
}

impl Peer for Link {
    async fn send(&mut self, reply: &Reply) -> Result<()> {
        self.tx.send(reply).await
    }

    async fn recv(&mut self) -> Result<Option<Request>> {
        self.rx.recv().await
    }
}

pub(super) async fn serve_with(terminals: &Terminals, peer: &mut impl Peer) -> Result<()> {
    while let Some(request) = peer.recv().await? {
        if let Request::Attach { id, from } = request {
            return match terminals.live(id) {
                Some(live) => attach(live, from, peer).await,
                None => peer.send(&unknown(id)).await,
            };
        }

        let reply = terminals.answer(request).await;
        peer.send(&reply).await?;
    }

    Ok(())
}

/// Streams terminal `live` from byte `from` and types what the peer sends
/// into it, until it ends or the peer closes the stream.
async fn attach(live: Arc<Live>, from: u64, peer: &mut impl Peer) -> Result<()> {
    let mut changed = live.changed.subscribe();
    let mut next = from;

    loop {
        // Marked seen before reading, so output that lands after the read
        // still wakes the wait below.
        watched::read_and_update(&mut changed, |_| ());
        let (chunk, state) = {
            let screen = lock(&live.screen);
            (screen.read_from(next), screen.phase.state())
        };

        if let Some((at, data)) = chunk {
            next = at + data.len() as u64;
            peer.send(&Reply::Output {
                at,
                data: ByteBuf::from(data),
            })
            .await?;
            continue;
        }

        if !state.is_running() {
            return peer.send(&Reply::Ended { task: live.task() }).await;
        }

        tokio::select! {
            woke = changed.changed() => if woke.is_err() {
                return Ok(());
            },
            request = peer.recv() => match request {
                Ok(Some(Request::Input { data })) if data.len() <= MAX_INPUT => {
                    live.type_keys(data.into_vec());
                }
                Ok(Some(Request::Input { data })) => {
                    log::debug!("dropped {} bytes of input past the limit", data.len());
                }
                Ok(Some(Request::Resize { size })) => {
                    live.resize(size);
                }
                Ok(Some(other)) => log::debug!("ignored {other:?} on an attached terminal"),
                // The peer detached.
                Ok(None) | Err(_) => return Ok(()),
            },
        }
    }
}
