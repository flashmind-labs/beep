//! Topic subscriptions on their own authenticated QUIC connection.

use super::{
    Offers, Session, channels, events,
    peers::{CHANNEL_OPEN_WAIT, Peers},
};
use anyhow::{Context, Result};
use beep_proto::node::{LinkRx, LinkTx, subscriptions::Open};
use iroh::{
    EndpointId,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler},
};
use std::fmt;
use tokio::time::timeout;

#[derive(Clone)]
pub struct SubscriptionsAlpn(pub(super) Peers);

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
    tx: LinkTx,
    rx: LinkRx,
) -> Session {
    Box::pin(async move {
        anyhow::ensure!(
            peers.controller_allowed(peer, mesh),
            "only a controller may subscribe"
        );
        serve_open(tx, rx, offers, peer).await
    })
}

pub(super) async fn serve_open(
    tx: LinkTx,
    mut rx: LinkRx,
    offers: Offers,
    caller: EndpointId,
) -> Result<()> {
    timeout(CHANNEL_OPEN_WAIT, rx.recv::<Open>())
        .await??
        .context("the subscription stream closed before opening")?;
    events::subscriptions::serve(tx, rx, offers, caller).await
}
