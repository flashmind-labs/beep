//! Topic subscriptions on their own authenticated QUIC connection.

use super::{
    Offers, Session, channels,
    events::{self, Reach},
    peers::{CHANNEL_OPEN_WAIT, Peers},
};
use anyhow::{Context, Result};
use beep_proto::node::{LinkRx, LinkTx, link, subscriptions::Open};
use iroh::{
    EndpointId,
    endpoint::{Connection, RecvStream, SendStream},
    protocol::{AcceptError, ProtocolHandler},
};
use std::fmt;
use tokio::time::timeout;

#[derive(Clone)]
pub struct SubscriptionsAlpn(Peers);

impl SubscriptionsAlpn {
    pub fn new(peers: &Peers) -> Self {
        Self(peers.clone())
    }
}

impl fmt::Debug for SubscriptionsAlpn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SubscriptionsAlpn")
    }
}

impl ProtocolHandler for SubscriptionsAlpn {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        channels::serve_connection(&self.0, connection, serve_stream).await
    }
}

fn serve_stream(
    peers: Peers,
    offers: Offers,
    peer: EndpointId,
    mesh: EndpointId,
    send: SendStream,
    recv: RecvStream,
) -> Session {
    Box::pin(async move {
        let (tx, rx) = link(send, recv);
        anyhow::ensure!(
            peers.controller_allowed(peer, mesh),
            "only a controller may subscribe"
        );
        serve_open(tx, rx, offers, peer, Reach::Whole).await
    })
}

pub(super) async fn serve_open(
    tx: LinkTx,
    mut rx: LinkRx,
    offers: Offers,
    caller: EndpointId,
    reach: Reach,
) -> Result<()> {
    let open = timeout(CHANNEL_OPEN_WAIT, rx.recv::<Open>())
        .await??
        .context("the subscription stream closed before opening")?;
    let compress = open.compress && !tx.is_zstd();
    events::serve_subscriptions(tx, rx, offers, caller, reach, compress).await
}
