//! Port forwards on their own authenticated QUIC connection: each stream
//! reaches a TCP port on this machine's loopback.

use super::peers::{CHANNEL_OPEN_WAIT, Peers};
use anyhow::{Context, Result};
use iroh::{
    EndpointId,
    endpoint::{RecvStream, SendStream},
};
use std::net::{Ipv4Addr, Ipv6Addr};
use tokio::{
    io::{AsyncReadExt, copy_bidirectional, join},
    net::TcpStream,
    time::timeout,
};

/// Only a controller may forward, as it may run commands here, and only
/// while this machine lends itself to peers.
pub(super) async fn serve_stream(
    peers: Peers,
    peer: EndpointId,
    mesh: EndpointId,
    mut send: SendStream,
    mut recv: RecvStream,
) -> Result<()> {
    let reached = async {
        anyhow::ensure!(
            peers.controller_allowed(peer, mesh),
            "only a controller may forward ports"
        );
        let port = timeout(CHANNEL_OPEN_WAIT, recv.read_u16())
            .await
            .context("the forward named no port")??;
        connect(port).await
    };
    let mut tcp = match reached.await {
        Ok(tcp) => tcp,
        Err(error) => {
            send.reset(1u32.into()).ok();
            return Err(error);
        }
    };

    copy_bidirectional(&mut join(recv, send), &mut tcp).await?;
    Ok(())
}

/// IPv4 loopback first, then IPv6: a dev server binds either.
async fn connect(port: u16) -> Result<TcpStream> {
    match TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await {
        Ok(tcp) => Ok(tcp),
        Err(_) => TcpStream::connect((Ipv6Addr::LOCALHOST, port))
            .await
            .with_context(|| format!("nothing listens on port {port}")),
    }
}
