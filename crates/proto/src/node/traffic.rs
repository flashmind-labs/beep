//! `Up::Traffic`: the bytes a node moved over iroh, to judge compression by.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Cumulative counters since the node's process started. Reported again in
/// full each time, so a lost message loses nothing; `started_at_ms` lets the
/// server tell a restart, which zeroes them, from a report it has seen part of.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrafficReport {
    /// When the process started, in milliseconds since the Unix epoch.
    pub started_at_ms: u64,
    /// Bytes by key: `alpn.<alpn>.tx` and `.rx` on the wire per protocol, with
    /// QUIC's overhead and retransmits; `endpoint.direct.tx` and
    /// `endpoint.relay.rx` and the like over all of the node's iroh sockets.
    pub counters: BTreeMap<String, u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{EnvReport, Up};

    #[test]
    fn traffic_round_trips_and_an_older_server_skips_it() {
        let up = Up::Traffic(TrafficReport {
            started_at_ms: 1_760_000_000_123,
            counters: BTreeMap::from([("alpn.beep-sync/1.tx".to_owned(), 42)]),
        });
        let body = rmp_serde::to_vec_named(&up).unwrap();
        assert_eq!(rmp_serde::from_slice::<Up>(&body).unwrap(), up);
        // A server from before `Up::Traffic` fails to read the message, which
        // its control loop skips.
        #[derive(Debug, Deserialize)]
        enum OldUp {
            #[allow(dead_code)]
            Env(EnvReport),
        }
        assert!(rmp_serde::from_slice::<OldUp>(&body).is_err());
    }
}
