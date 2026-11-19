//! The DNS resolver of every endpoint: encrypted public DNS first, the
//! system's nameservers only when that fails. Some networks rewrite plain DNS
//! answers, such as a hotel filter that sends every iroh relay name to its
//! block page. A rewritten answer is not a failure, so racing the system's
//! nameservers or keeping them first lets it win.
//!
//! An answer from encrypted DNS is final, "no such name" and "no records"
//! included. Only a timeout, an unreachable server or another failure asks
//! the system, so a rewriting network cannot answer names public DNS says do
//! not exist.

use futures::future::BoxFuture;
use iroh::dns::{BoxIter, DnsError, DnsResolver, NameserverConfig, Resolver, TxtRecordData};
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    time::Duration,
};

/// Public resolvers whose certificates name their addresses, so DNS over TLS
/// and HTTPS need no lookup of their own.
const PUBLIC: [IpAddr; 6] = [
    IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
    IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
    IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9)),
    IpAddr::V6(Ipv6Addr::new(0x2606, 0x4700, 0x4700, 0, 0, 0, 0, 0x1111)),
    IpAddr::V6(Ipv6Addr::new(0x2001, 0x4860, 0x4860, 0, 0, 0, 0, 0x8888)),
    IpAddr::V6(Ipv6Addr::new(0x2620, 0xfe, 0, 0, 0, 0, 0, 0xfe)),
];
/// How long encrypted DNS has before the system's nameservers are asked.
/// iroh gives a whole lookup three seconds, so the system keeps the rest.
const ENCRYPTED_WAIT: Duration = Duration::from_millis(1500);
/// The system's share of the lookup.
const SYSTEM_WAIT: Duration = Duration::from_millis(1500);

/// The resolver to give an endpoint builder.
pub fn resolver() -> DnsResolver {
    DnsResolver::custom(EncryptedFirst::new())
}

#[derive(Debug)]
struct EncryptedFirst {
    encrypted: DnsResolver,
    system: DnsResolver,
}

impl EncryptedFirst {
    fn new() -> Self {
        // HTTPS before TLS: the resolver starts the first few servers in this
        // order, and a network that drops port 853 must not hold every slot.
        let encrypted = PUBLIC
            .into_iter()
            .map(NameserverConfig::https)
            .chain(PUBLIC.into_iter().map(NameserverConfig::tls));

        Self {
            encrypted: DnsResolver::builder()
                .add_nameserver_configs(encrypted)
                .disable_fallback()
                .build(),
            system: DnsResolver::new(),
        }
    }
}

/// Whether `error` means encrypted DNS got no answer, so the system should be
/// asked. An answer, even "no such name", is final.
fn unanswered(error: &DnsError) -> bool {
    !matches!(error, DnsError::NxDomain { .. })
}

/// Runs `lookup` on the encrypted resolver, then on the system's when that got
/// no answer, keeping the records `pick` takes.
macro_rules! encrypted_first {
    ($self:ident, $lookup:ident, $host:ident, $pick:expr) => {{
        let encrypted = $self.encrypted.clone();
        let system = $self.system.clone();
        Box::pin(async move {
            let found = match encrypted.$lookup(&$host, ENCRYPTED_WAIT).await {
                Ok(found) => found.filter_map($pick).collect::<Vec<_>>(),
                Err(e) if unanswered(&e) => {
                    log::debug!("encrypted DNS failed for {}, asking the system: {e}", $host);
                    system
                        .$lookup(&$host, SYSTEM_WAIT)
                        .await?
                        .filter_map($pick)
                        .collect()
                }
                Err(e) => return Err(e),
            };
            let found: BoxIter<_> = Box::new(found.into_iter());
            Ok(found)
        })
    }};
}

impl Resolver for EncryptedFirst {
    fn lookup_ipv4(&self, host: String) -> BoxFuture<'static, Result<BoxIter<Ipv4Addr>, DnsError>> {
        encrypted_first!(self, lookup_ipv4, host, |ip| match ip {
            IpAddr::V4(ip) => Some(ip),
            IpAddr::V6(_) => None,
        })
    }

    fn lookup_ipv6(&self, host: String) -> BoxFuture<'static, Result<BoxIter<Ipv6Addr>, DnsError>> {
        encrypted_first!(self, lookup_ipv6, host, |ip| match ip {
            IpAddr::V6(ip) => Some(ip),
            IpAddr::V4(_) => None,
        })
    }

    fn lookup_txt(
        &self,
        host: String,
    ) -> BoxFuture<'static, Result<BoxIter<TxtRecordData>, DnsError>> {
        encrypted_first!(self, lookup_txt, host, Some)
    }

    fn clear_cache(&self) {
        self.encrypted.clear_cache();
        self.system.clear_cache();
    }

    /// A network change brings new system nameservers and new sockets.
    fn reset(&self) -> Box<dyn Resolver> {
        Box::new(Self::new())
    }
}

#[cfg(test)]
#[path = "tests/dns.rs"]
mod tests;
