//! URL validation for custom MCP servers.

use reqwest::Url;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Why a URL is refused, worded for the user.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum UrlError {
    #[error("The address must be a full https:// URL.")]
    NotHttps,
    #[error("The address must not carry a user name or password.")]
    Credentials,
    #[error("Beep could not find that server.")]
    Unresolved,
    #[error("That address is on a private network, which Beep cannot reach.")]
    Private,
}

/// The URL when it is https without credentials and every address of its host is public.
pub(crate) async fn check_url(url: &str) -> Result<Url, UrlError> {
    let url = Url::parse(url.trim()).map_err(|_| UrlError::NotHttps)?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err(UrlError::NotHttps);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(UrlError::Credentials);
    }
    let host = url.host_str().ok_or(UrlError::NotHttps)?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let port = url.port_or_known_default().unwrap_or(443);
    let addrs: Vec<IpAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| UrlError::Unresolved)?
        .map(|addr| addr.ip())
        .collect();
    if addrs.is_empty() {
        return Err(UrlError::Unresolved);
    }
    if !addrs.into_iter().all(is_public) {
        return Err(UrlError::Private);
    }
    Ok(url)
}

/// Whether `ip` is public; IPv6 must be global unicast (2000::/3), which excludes mesh ranges.
pub(super) fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => public_v4(ip),
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return public_v4(v4);
            }
            let first = ip.segments()[0];
            let global_unicast = first & 0xe000 == 0x2000;
            let documentation = first == 0x2001 && ip.segments()[1] == 0x0db8;
            // 6to4 and Teredo carry an IPv4 address the check above did not see.
            let tunnel = first == 0x2002 || (first == 0x2001 && ip.segments()[1] == 0);
            global_unicast && !documentation && !tunnel && ip != Ipv6Addr::UNSPECIFIED
        }
    }
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || ip.is_unspecified()
        || a == 0
        || a >= 240
        // Shared address space (carrier-grade NAT).
        || (a == 100 && (64..128).contains(&b))
        // Benchmarking.
        || (a == 198 && (18..20).contains(&b)))
}

/// The host a call's limits are counted on.
pub(crate) fn host_of(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
        .unwrap_or_default()
}
