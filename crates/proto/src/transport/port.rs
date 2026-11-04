//! Fixed UDP ports for iroh endpoints, so a firewall can let them through.

use iroh::{
    Endpoint,
    endpoint::{BindError, BindOpts, Builder, QuicTransportConfig, QuicTransportConfigBuilder},
};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};

/// The UDP port every beep-node binds unless `node.toml` names another.
pub const NODE_PORT: u16 = 47120;
/// The UDP port the server binds unless its config names another; not the
/// node's, since a node often runs beside it.
pub const SERVER_PORT: u16 = 47121;

/// The transport of everything that stays connected. It sends each datagram on
/// its own: on a network card without checksum offload, batches fail. The socket
/// notices at the first one, but iroh gives each connection the batch size as it
/// was when the connection was made, so every older one goes on sending batches
/// that are dropped without a word. Anything larger than a datagram then waits
/// for retries, for seconds or minutes on a connection that looks healthy.
pub fn transport() -> QuicTransportConfigBuilder {
    QuicTransportConfig::builder().enable_segmentation_offload(false)
}

/// Binds the endpoint `builder` makes on UDP `port`, IPv4 and, where the
/// machine has it, IPv6. A port that is taken gets a warning and a random
/// one instead; 0 is random from the start.
pub async fn bind_on(builder: impl Fn() -> Builder, port: u16) -> Result<Endpoint, BindError> {
    if port != 0 {
        let fixed = builder()
            .clear_ip_transports()
            .bind_addr(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)))
            .and_then(|fixed| {
                fixed.bind_addr_with_opts(
                    SocketAddr::from((Ipv6Addr::UNSPECIFIED, port)),
                    BindOpts::default().set_is_required(false),
                )
            });
        match fixed {
            Ok(fixed) => match fixed.bind().await {
                Ok(endpoint) => return Ok(endpoint),
                Err(e) => log::warn!("UDP port {port} is not free, binding a random one: {e}"),
            },
            Err(e) => log::warn!("cannot bind UDP port {port}, binding a random one: {e}"),
        }
    }
    builder().bind().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::endpoint::presets;
    use std::net::UdpSocket;

    #[tokio::test]
    async fn binds_the_port_asked_for_or_a_random_one_when_it_is_taken() {
        let taken = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).expect("a free port binds");
        let port = taken
            .local_addr()
            .expect("a bound socket has an address")
            .port();
        let builder = || Endpoint::builder(presets::Minimal);

        let fallback = bind_on(builder, port).await.expect("falls back");
        assert!(
            fallback
                .bound_sockets()
                .iter()
                .all(|addr| addr.port() != port)
        );
        fallback.close().await;

        drop(taken);
        let fixed = bind_on(builder, port).await.expect("binds the free port");
        assert!(
            fixed
                .bound_sockets()
                .iter()
                .any(|addr| addr.is_ipv4() && addr.port() == port)
        );
        fixed.close().await;
    }
}
