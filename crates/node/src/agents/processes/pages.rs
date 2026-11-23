//! Which listening ports serve a page, so the desktop can offer the one a
//! dev server shows instead of every port its processes hold.

use futures::{StreamExt, stream::FuturesUnordered};
use reqwest::{Client, redirect::Policy};
use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::LazyLock,
    time::Duration,
};

/// How long a port gets to answer; a dev server on loopback answers at once.
const WAIT: Duration = Duration::from_millis(500);

static CLIENT: LazyLock<Option<Client>> = LazyLock::new(|| {
    beep_proto::https::client()
        .no_proxy()
        .redirect(Policy::none())
        .timeout(WAIT)
        .build()
        .inspect_err(|e| log::debug!("no client to test ports: {e:#}"))
        .ok()
});

/// Each of `ports` with whether it answers `GET /` with a page or a redirect,
/// on IPv4 loopback or else IPv6 loopback, which is where a forward reaches.
/// `None` when it has not settled: it timed out or answered a server error,
/// as a dev server may while it starts.
pub(super) async fn test(ports: impl IntoIterator<Item = u16>) -> Vec<(u16, Option<bool>)> {
    let Some(client) = CLIENT.as_ref() else {
        return Vec::new();
    };

    ports
        .into_iter()
        .map(|port| async move { (port, page(client, port).await) })
        .collect::<FuturesUnordered<_>>()
        .collect()
        .await
}

async fn page(client: &Client, port: u16) -> Option<bool> {
    for ip in [Ipv4Addr::LOCALHOST.into(), Ipv6Addr::LOCALHOST.into()] {
        let url = format!("http://{}/", SocketAddr::new(ip, port));

        match client.get(url).send().await {
            Ok(reply) if reply.status().is_server_error() => return None,
            Ok(reply) => {
                let status = reply.status();
                return Some(status.is_success() || status.is_redirection());
            }
            Err(e) if e.is_connect() => continue,
            Err(e) if e.is_timeout() => return None,
            Err(_) => return Some(false),
        }
    }

    Some(false)
}
