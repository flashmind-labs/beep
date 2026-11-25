//! Port forwards on their own authenticated QUIC connection: each stream
//! reaches a TCP port on this machine's loopback.

use super::{
    Offers, Session, channels,
    peers::{CHANNEL_OPEN_WAIT, Peers},
};
use anyhow::{Context, Result};
use iroh::{
    EndpointId,
    endpoint::{Connection, RecvStream, SendStream},
    protocol::{AcceptError, ProtocolHandler},
};
use std::{
    fmt,
    net::{Ipv4Addr, Ipv6Addr},
};
use tokio::{
    io::{AsyncReadExt, copy_bidirectional, join},
    net::TcpStream,
    time::timeout,
};

#[derive(Clone)]
pub struct ForwardAlpn(Peers);

impl ForwardAlpn {
    pub fn new(peers: &Peers) -> Self {
        Self(peers.clone())
    }
}

impl fmt::Debug for ForwardAlpn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ForwardAlpn")
    }
}

impl ProtocolHandler for ForwardAlpn {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        channels::serve_connection(&self.0, connection, serve_stream).await
    }
}

/// Only a controller may forward, as it may run commands here, and only
/// while this machine lends itself to peers.
fn serve_stream(
    peers: Peers,
    _: Offers,
    peer: EndpointId,
    mesh: EndpointId,
    mut send: SendStream,
    mut recv: RecvStream,
) -> Session {
    Box::pin(async move {
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
    })
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
