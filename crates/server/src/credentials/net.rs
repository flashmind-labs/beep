//! Which addresses the server may send a user's token to. A Forgejo URL is
//! the user's to type, so without this the server could be pointed at
//! itself, its cloud's metadata service or another host on its network.

use reqwest::{
    Url,
    dns::{Addrs, Name, Resolve, Resolving},
};
use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
};
use url::Host;

/// Whether `ip` is on the public internet: not loopback, private,
/// link-local (where cloud metadata services answer), unique local, shared
/// (CGNAT), multicast or otherwise reserved.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_public_v4(v4),
            None => is_public_v6(v6),
        },
    }
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

/// Whether every address `url`'s host has is public. `false` when the
/// name does not resolve.
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

/// A resolver for the client that probes tokens: it gives only public
/// addresses, so a name that resolves elsewhere by the time of the request
/// fails instead of reaching inside.
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
mod tests {
    use super::*;

    #[test]
    fn inside_addresses_are_not_public() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fd00:ec2::254",
            "fe80::1",
            "::ffff:10.0.0.1",
        ] {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["1.2.3.4", "2a00:1::1"] {
            assert!(is_public(ip.parse().unwrap()), "{ip}");
        }
    }
}
