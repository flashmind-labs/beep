use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use n0_error::e;

use super::*;

const REAL: Ipv4Addr = Ipv4Addr::new(192, 0, 2, 1);
const REWRITTEN: Ipv4Addr = Ipv4Addr::new(198, 51, 100, 1);

/// What a [`Fixed`] resolver says to every A query.
#[derive(Debug, Clone, Copy)]
enum Says {
    Address(Ipv4Addr),
    NoRecords,
    NoSuchName,
    Nothing,
}

/// Gives `says` to every A query and counts the queries it saw.
#[derive(Debug, Clone)]
struct Fixed {
    says: Says,
    queries: Arc<AtomicUsize>,
}

impl Resolver for Fixed {
    fn lookup_ipv4(
        &self,
        _host: String,
    ) -> BoxFuture<'static, Result<BoxIter<Ipv4Addr>, DnsError>> {
        self.queries.fetch_add(1, Ordering::SeqCst);
        let says = self.says;

        Box::pin(async move {
            match says {
                Says::Address(ip) => Ok(Box::new(std::iter::once(ip)) as BoxIter<_>),
                Says::NoRecords => Ok(Box::new(std::iter::empty()) as BoxIter<_>),
                Says::NoSuchName => Err(e!(DnsError::NxDomain)),
                Says::Nothing => Err(e!(DnsError::Timeout)),
            }
        })
    }

    fn lookup_ipv6(
        &self,
        _host: String,
    ) -> BoxFuture<'static, Result<BoxIter<Ipv6Addr>, DnsError>> {
        Box::pin(async { Err(e!(DnsError::Timeout)) })
    }

    fn lookup_txt(
        &self,
        _host: String,
    ) -> BoxFuture<'static, Result<BoxIter<TxtRecordData>, DnsError>> {
        Box::pin(async { Err(e!(DnsError::Timeout)) })
    }

    fn clear_cache(&self) {}

    fn reset(&self) -> Box<dyn Resolver> {
        Box::new(self.clone())
    }
}

/// Looks up an A record with `encrypted` and `system`, returning what was
/// found and how many queries the system saw.
async fn lookup(encrypted: Says, system: Says) -> (Result<Vec<Ipv4Addr>, DnsError>, usize) {
    let fixed = |says| {
        let queries = Arc::new(AtomicUsize::new(0));
        let resolver = DnsResolver::custom(Fixed {
            says,
            queries: queries.clone(),
        });
        (resolver, queries)
    };
    let (encrypted, _) = fixed(encrypted);
    let (system, system_queries) = fixed(system);

    let found = DnsResolver::custom(EncryptedFirst { encrypted, system })
        .lookup_ipv4("relay.example", Duration::from_secs(3))
        .await
        .map(|found| {
            found
                .filter_map(|ip| match ip {
                    IpAddr::V4(ip) => Some(ip),
                    IpAddr::V6(_) => None,
                })
                .collect()
        });

    (found, system_queries.load(Ordering::SeqCst))
}

#[tokio::test]
async fn an_encrypted_answer_beats_a_rewritten_one() {
    let (found, asked) = lookup(Says::Address(REAL), Says::Address(REWRITTEN)).await;

    assert_eq!(found.expect("encrypted DNS answers"), [REAL]);
    assert_eq!(asked, 0);
}

#[tokio::test]
async fn the_system_answers_when_encrypted_dns_times_out() {
    let (found, asked) = lookup(Says::Nothing, Says::Address(REAL)).await;

    assert_eq!(found.expect("the system answers"), [REAL]);
    assert_eq!(asked, 1);
}

#[tokio::test]
async fn an_encrypted_negative_answer_is_final() {
    for says in [Says::NoRecords, Says::NoSuchName] {
        let (found, asked) = lookup(says, Says::Address(REWRITTEN)).await;

        assert!(found.map_or(true, |found| found.is_empty()), "{says:?}");
        assert_eq!(asked, 0, "{says:?}");
    }
}
