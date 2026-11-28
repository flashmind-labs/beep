//! Topic subscriptions on their own authenticated QUIC connection.

use super::{
    Offers,
    events::{self, Reach},
    peers::{CHANNEL_OPEN_WAIT, Peers},
};
use anyhow::{Context, Result};
use beep_proto::node::{LinkRx, LinkTx, link, subscriptions::Open};
use iroh::{
    EndpointId,
    endpoint::{RecvStream, SendStream},
};
use tokio::time::timeout;

pub(super) async fn serve_stream(
    peers: Peers,
    offers: Offers,
    peer: EndpointId,
    mesh: EndpointId,
    send: SendStream,
    recv: RecvStream,
) -> Result<()> {
    let (tx, rx) = link(send, recv);
    anyhow::ensure!(
        peers.controller_allowed(peer, mesh),
        "only a controller may subscribe"
    );

    serve_open(tx, rx, offers, peer, false, Reach::Whole).await
}

pub(super) async fn serve_open(
    tx: LinkTx,
    mut rx: LinkRx,
    offers: Offers,
    caller: EndpointId,
    local: bool,
    reach: Reach,
) -> Result<()> {
    let open = timeout(CHANNEL_OPEN_WAIT, rx.recv::<Open>())
        .await??
        .context("the subscription stream closed before opening")?;
    let compress = open.compress && !tx.is_zstd();
    events::serve_subscriptions(tx, rx, offers, caller, local, reach, compress).await
}
