//! Bytes in and out per ALPN, for `Up::Traffic`.
//!
//! An endpoint hook sees every connection after its handshake, in both
//! directions and whoever owns it, so iroh-blobs' connections count with the
//! rest. An endpoint without the hook, such as the one the Mac app shares with
//! its own calls, counts only what `watch` is given: the node link.
//!
//! Not counted: handshakes that fail, and what iroh sends outside connections
//! (relay keepalives, address discovery). `endpoint.*` includes those.

use crate::lock;
use beep_proto::node::traffic::TrafficReport;
use futures::FutureExt;
use iroh::{
    endpoint::{
        AfterHandshakeOutcome, Closed, Connection, ConnectionStats, EndpointHooks,
        WeakConnectionHandle,
    },
    metrics::EndpointMetrics,
};
use std::{
    collections::BTreeMap,
    fmt,
    future::{self, Future},
    pin::Pin,
    sync::{Arc, Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

/// A node's counters. Clones share them; they start at zero with the process.
#[derive(Clone)]
pub struct Traffic(Arc<Shared>);

struct Shared {
    started_at_ms: u64,
    endpoint: OnceLock<EndpointMetrics>,
    tally: Mutex<Tally>,
}

#[derive(Default)]
struct Tally {
    /// What the connections that ended moved, by ALPN.
    ended: BTreeMap<Vec<u8>, Bytes>,
    live: Vec<Live>,
}

/// A connection still open. Holds it weakly, so it closes as if never watched.
struct Live {
    id: usize,
    alpn: Vec<u8>,
    handle: WeakConnectionHandle,
    /// Gives the final stats, after the last handle to the connection is gone.
    closed: Pin<Box<dyn Future<Output = Option<Closed>> + Send>>,
    /// The stats last read, for a connection dropped but not yet reported closed.
    last: Bytes,
}

#[derive(Clone, Copy, Default)]
struct Bytes {
    tx: u64,
    rx: u64,
}

impl Bytes {
    fn add(&mut self, other: Bytes) {
        self.tx += other.tx;
        self.rx += other.rx;
    }
}

impl From<&ConnectionStats> for Bytes {
    fn from(stats: &ConnectionStats) -> Self {
        Self {
            tx: stats.udp_tx.bytes,
            rx: stats.udp_rx.bytes,
        }
    }
}

impl Tally {
    /// Moves the connections that closed to `ended` and reads the others.
    fn settle(&mut self) {
        let ended = &mut self.ended;
        self.live.retain_mut(|live| {
            match (&mut live.closed).now_or_never() {
                None => {
                    if let Some(connection) = live.handle.upgrade() {
                        live.last = (&connection.stats()).into();
                    }
                    return true;
                }
                Some(Some(closed)) => live.last = (&closed.stats).into(),
                Some(None) => {}
            }
            ended.entry(live.alpn.clone()).or_default().add(live.last);
            false
        });
    }
}

impl Default for Traffic {
    fn default() -> Self {
        let started_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|since| u64::try_from(since.as_millis()).ok())
            .unwrap_or_default();
        Self(Arc::new(Shared {
            started_at_ms,
            endpoint: OnceLock::new(),
            tally: Mutex::default(),
        }))
    }
}

impl Traffic {
    /// Adds the totals of the endpoint this node alone uses. Call it once.
    pub fn measure_endpoint(&self, metrics: EndpointMetrics) {
        if self.0.endpoint.set(metrics).is_err() {
            log::warn!("traffic already measures an endpoint");
        }
    }

    /// Counts `connection` from now on, and on past its closing. Watching it
    /// again changes nothing.
    pub fn watch(&self, connection: &Connection) {
        let id = connection.stable_id();
        let mut tally = lock(&self.0.tally);
        // A hook runs for each connection, so this also clears the closed
        // ones while no link asks for reports.
        tally.settle();
        if tally.live.iter().any(|live| live.id == id) {
            return;
        }
        let handle = connection.weak_handle();
        tally.live.push(Live {
            id,
            alpn: connection.alpn().to_vec(),
            closed: Box::pin(handle.closed()),
            handle,
            last: (&connection.stats()).into(),
        });
    }

    /// The counters as they stand.
    pub fn report(&self) -> TrafficReport {
        let by_alpn = {
            let mut tally = lock(&self.0.tally);
            tally.settle();
            let mut by_alpn = tally.ended.clone();
            for live in &tally.live {
                by_alpn.entry(live.alpn.clone()).or_default().add(live.last);
            }
            by_alpn
        };
        let mut counters = BTreeMap::new();
        for (alpn, bytes) in &by_alpn {
            let alpn = String::from_utf8_lossy(alpn);
            counters.insert(format!("alpn.{alpn}.tx"), bytes.tx);
            counters.insert(format!("alpn.{alpn}.rx"), bytes.rx);
        }
        if let Some(metrics) = self.0.endpoint.get() {
            let socket = &metrics.socket;
            for (key, bytes) in [
                (
                    "endpoint.direct.tx",
                    socket.send_ipv4.get() + socket.send_ipv6.get(),
                ),
                (
                    "endpoint.direct.rx",
                    socket.recv_data_ipv4.get() + socket.recv_data_ipv6.get(),
                ),
                ("endpoint.relay.tx", socket.send_relay.get()),
                ("endpoint.relay.rx", socket.recv_data_relay.get()),
            ] {
                counters.insert(key.to_owned(), bytes);
            }
        }
        TrafficReport {
            started_at_ms: self.0.started_at_ms,
            counters,
        }
    }
}

impl EndpointHooks for Traffic {
    fn after_handshake<'a>(
        &'a self,
        connection: &'a Connection,
    ) -> impl Future<Output = AfterHandshakeOutcome> + Send + 'a {
        self.watch(connection);
        future::ready(AfterHandshakeOutcome::accept())
    }
}

impl fmt::Debug for Traffic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Traffic")
            .field("started_at_ms", &self.0.started_at_ms)
            .finish_non_exhaustive()
    }
}
