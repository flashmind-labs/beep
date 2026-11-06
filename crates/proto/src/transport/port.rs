//! Fixed UDP ports for iroh endpoints, so a firewall can let them through,
//! and the settings every device's endpoint has.

use iroh::{
    Endpoint, SecretKey,
    endpoint::{
        BindError, BindOpts, Builder, NetReportConfig, PortmapperConfig, QuicTransportConfig,
        QuicTransportConfigBuilder, presets,
    },
};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};

/// The UDP port every beep-node binds unless `node.toml` names another.
pub const NODE_PORT: u16 = 47120;
/// The UDP port the server binds unless its config names another; not the
/// node's, since a node often runs beside it.
pub const SERVER_PORT: u16 = 47121;
/// The TCP port the server serves the apps' HTTP on unless its config names
/// another.
pub const SERVER_HTTP_PORT: u16 = 47122;

/// Close code the server gives a node key the account removed.
pub const CLOSE_REVOKED: u32 = 403;

/// Binds an endpoint as `identity` that serves nothing yet: the caller
/// serves its protocols through a `Router`. HTTPS probes stay on, since
/// without them a network that blocks QUIC never finds a home relay.
pub async fn bind_endpoint(
    identity: SecretKey,
    transport: QuicTransportConfig,
) -> anyhow::Result<Endpoint> {
    let mut net_report = NetReportConfig::default();
    net_report.captive_portal_check = false;
    Ok(Endpoint::builder(presets::N0)
        .secret_key(identity)
        // UPnP and PCP never work on cellular, and each probe wakes the radio.
        .portmapper_config(PortmapperConfig::Disabled)
        .net_report_config(net_report)
        .transport_config(transport)
        .bind()
        .await?)
}

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
