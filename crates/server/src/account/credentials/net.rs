//! Which addresses the server may send a user's token to, so a typed URL cannot reach inside its network.

use reqwest::{
    Url,
    dns::{Addrs, Name, Resolve, Resolving},
};
use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
};
use url::Host;

/// Whether `ip` is on the public internet (excludes link-local, where cloud metadata answers).
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => match embedded_v4(v6) {
            Some(v4) => is_public_v4(v4),
            None => is_public_v6(v6),
        },
    }
}

/// The IPv4 address carried by a mapped, compatible, NAT64 or 6to4 IPv6 address.
fn embedded_v4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return Some(v4);
    }

    let seg = ip.segments();
    let octets = ip.octets();
    let last = Ipv4Addr::new(octets[12], octets[13], octets[14], octets[15]);

    if seg[..6] == [0; 6] && !ip.is_loopback() && !ip.is_unspecified() {
        return Some(last);
    }

    if seg[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return Some(last);
    }

    if seg[0] == 0x2002 {
        return Some(Ipv4Addr::new(octets[2], octets[3], octets[4], octets[5]));
    }

    None
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || a == 0
        // Shared address space, used for carrier-grade NAT and mesh VPNs.
        || (a == 100 && (64..128).contains(&b))
        // IETF protocol assignments and benchmarking.
        || (a == 192 && b == 0 && ip.octets()[2] == 0)
        || (a == 198 && (b == 18 || b == 19))
        || a >= 240)
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        // Unique local, fc00::/7, which also holds cloud metadata addresses.
        || (first & 0xfe00) == 0xfc00
        // Link-local, fe80::/10.
        || (first & 0xffc0) == 0xfe80
        // Documentation, 2001:db8::/32.
        || (first == 0x2001 && ip.segments()[1] == 0x0db8))
}

/// Whether every address of `url`'s host is public; false when it does not resolve.
pub async fn resolves_public(url: &Url) -> bool {
    let port = url.port_or_known_default().unwrap_or(443);

    match url.host() {
        Some(Host::Ipv4(ip)) => is_public(IpAddr::V4(ip)),
        Some(Host::Ipv6(ip)) => is_public(IpAddr::V6(ip)),
        Some(Host::Domain(name)) => match tokio::net::lookup_host((name, port)).await {
            Ok(addrs) => {
                let addrs: Vec<SocketAddr> = addrs.collect();
                !addrs.is_empty() && addrs.iter().all(|addr| is_public(addr.ip()))
            }
            Err(_) => false,
        },
        None => false,
    }
}

/// A resolver that returns only public addresses, so rebinding fails instead of reaching inside.
pub struct PublicOnly;

impl Resolve for PublicOnly {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let found = tokio::net::lookup_host((name.as_str(), 0)).await?;
            let public: Vec<SocketAddr> = found.filter(|addr| is_public(addr.ip())).collect();

            if public.is_empty() {
                let refused = io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("{} has no public address", name.as_str()),
                );
                return Err(Box::new(refused) as Box<dyn std::error::Error + Send + Sync>);
            }

            Ok(Box::new(public.into_iter()) as Addrs)
        })
    }
}

#[cfg(test)]
#[path = "../../tests/account_credentials_net.rs"]
mod tests;
