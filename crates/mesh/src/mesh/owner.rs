//! Owns peer dialers and retires connections after membership changes.

use crate::inner::Inner;
use crate::link;
use crate::timeouts::{OPEN_TIMEOUT, RETRY};
use crate::wire::{PEER_ALPN, ways};
use beep_proto::watched;
use iroh::endpoint::Connection;
use iroh::{EndpointAddr, EndpointId, Watcher};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::task::{AbortHandle, JoinSet};
use tokio::time::{Instant, sleep, timeout};
use tracing::{info, warn};

/// A connection that lasted this long counts as having worked.
const STABLE: Duration = Duration::from_secs(5);
/// Longest pause between dials to an offline member; an address or network
/// change ends the pause early and resets it to `RETRY`.
const MAX_RETRY: Duration = Duration::from_secs(300);
/// A removed peer's link may take this long to deliver the news before it is cut.
const GRACE: Duration = Duration::from_secs(3);
/// The larger id lets the smaller dial first, so one dial does the work.
const HEAD_START: Duration = Duration::from_millis(300);
/// How often the sleep watch reads the clocks.
const TICK: Duration = Duration::from_secs(5);
/// A tick this much later than due means the machine slept.
const SLEPT: Duration = Duration::from_secs(20);

/// A connection `own` cuts, and whether its mesh link tells the peer first.
struct Doomed {
    conn: Connection,
    told: bool,
}

/// Keeps one dialer per other member and cuts the links of devices that
/// are no longer members.
pub(super) async fn own(inner: Arc<Inner>) {
    let mut dialers: JoinSet<EndpointId> = JoinSet::new();
    let mut running: BTreeMap<EndpointId, AbortHandle> = BTreeMap::new();
    let mut closers: JoinSet<usize> = JoinSet::new();
    let mut cutting: BTreeSet<usize> = BTreeSet::new();
    let mut rev = inner.rev.subscribe();
    // Ends with this task, which the mesh aborts when it closes.
    let mut watch = JoinSet::new();
    watch.spawn(watch_sleep(inner.clone()));

    loop {
        watched::read_and_update(&mut rev, |_| ());
        inner.prune_channels();
        let view = inner.view();
        let member = view.devices.is_member(inner.me());
        let wanted: Vec<EndpointId> = if member {
            view.devices
                .members()
                .into_iter()
                .map(|d| d.id)
                .filter(|id| *id != inner.me())
                .collect()
        } else {
            for handle in running.values() {
                handle.abort();
            }

            Vec::new()
        };
        // A dialer for a removed member ends on its own once its link has
        // told the peer; aborting it would cut that short.
        running.retain(|id, _| wanted.contains(id));

        for id in wanted {
            running
                .entry(id)
                .or_insert_with(|| dialers.spawn(dial(inner.clone(), id)));
        }

        let doomed: Vec<Doomed> = inner
            .conns()
            .iter()
            .filter(|(id, _)| !member || !view.devices.is_member(**id))
            .map(|(_, live)| Doomed {
                conn: live.conn.clone(),
                told: true,
            })
            .chain(
                inner
                    .retired()
                    .values()
                    .filter(|retired| !member || !view.devices.is_member(retired.conn.remote_id()))
                    .map(|retired| Doomed {
                        conn: retired.conn.clone(),
                        told: false,
                    }),
            )
            .collect();
        for Doomed { conn, told } in doomed {
            let id = conn.stable_id();

            if cutting.insert(id) {
                // The link tells the peer first when it can; this ends it
                // when the peer will not read, and with it the peer's streams.
                let wait = if member && told {
                    GRACE
                } else {
                    Duration::ZERO
                };
                closers.spawn(async move {
                    sleep(wait).await;
                    conn.close(2u32.into(), b"removed");
                    id
                });
            }
        }

        tokio::select! {
            changed = rev.changed() => {
                if changed.is_err() {
                    return;
                }
            }
            Some(ended) = dialers.join_next() => match ended {
                Ok(id) => { running.remove(&id); }
                Err(e) if e.is_cancelled() => {}
                Err(e) => warn!("a dialer failed: {e}"),
            },
            Some(ended) = closers.join_next() => {
                if let Ok(id) = ended {
                    cutting.remove(&id);
                }
            }
        }
    }
}

/// Keeps a connection to `peer` up while it is a member and so are we.
#[tracing::instrument(skip_all, fields(peer = %peer.fmt_short()))]
async fn dial(inner: Arc<Inner>, peer: EndpointId) -> EndpointId {
    let mut waited = false;
    let mut backoff = RETRY;
    // Our own addresses: a change means the network came back or moved.
    let mut ours = inner.endpoint.watch_addr();
    let mut network = true;

    loop {
        let notified = inner.wake.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();

        if !inner.is_member(peer) || !inner.is_member(inner.me()) || inner.is_closed() {
            return peer;
        }

        if let Some(conn) = inner.live(peer) {
            backoff = RETRY;
            tokio::select! {
                () = &mut notified => {}
                _ = conn.closed() => {}
            }
            continue;
        }

        if inner.me() > peer && !waited {
            waited = true;
            tokio::select! {
                () = &mut notified => {}
                () = sleep(HEAD_START) => {}
            }
            continue;
        }

        let tried = inner.addrs().get(&peer).cloned();
        let addr = tried
            .as_ref()
            .map_or_else(|| EndpointAddr::new(peer), |n| n.addr.clone());
        let to = ways(&addr);
        let started = Instant::now();
        let connect = timeout(OPEN_TIMEOUT, inner.endpoint.connect(addr, PEER_ALPN));
        tokio::pin!(connect);
        // Unrelated changes wake us; only a link that appeared cancels the dial.
        let dialed = loop {
            tokio::select! {
                dialed = &mut connect => break Some(dialed),
                () = &mut notified => {
                    if inner.live(peer).is_some() {
                        break None;
                    }

                    notified.set(inner.wake.notified());
                    notified.as_mut().enable();
                }
            }
        };
        let Some(dialed) = dialed else {
            backoff = RETRY;
            continue;
        };

        match dialed {
            Ok(Ok(conn)) => {
                link::serve(inner.clone(), conn, inner.me()).await;
                waited = false;

                if started.elapsed() >= STABLE {
                    backoff = RETRY;
                    continue;
                }
            }
            Ok(Err(e)) => {
                info!(
                    elapsed_ms = started.elapsed().as_millis(),
                    "dial to {to} failed: {e}"
                )
            }
            Err(_) => info!("dial to {to} timed out"),
        }
        // A member that stays away is tried with exponential backoff up to
        // `MAX_RETRY`. One that comes back dials us itself, and a new address
        // for it or for us, as after a sleep or a Wi-Fi drop, ends the wait early.
        let pause = sleep(backoff);
        tokio::pin!(pause);
        let mut timed_out = false;

        loop {
            let moved = inner.addr_wake.notified();
            tokio::pin!(moved);
            moved.as_mut().enable();
            let woke = inner.wake.notified();
            tokio::pin!(woke);
            woke.as_mut().enable();

            if inner.is_closed() || !inner.is_member(peer) || !inner.is_member(inner.me()) {
                return peer;
            }

            let now = inner.addrs().get(&peer).cloned();

            if now != tried {
                backoff = RETRY;
                break;
            }

            if inner.live(peer).is_some() {
                backoff = RETRY;
                break;
            }

            tokio::select! {
                () = &mut pause => {
                    timed_out = true;
                    break;
                }
                () = &mut moved => {}
                () = &mut woke => {}
                changed = ours.updated(), if network => {
                    if changed.is_ok() {
                        backoff = RETRY;
                        break;
                    }

                    network = false;
                }
            }
        }

        if timed_out {
            backoff = (backoff * 2).min(MAX_RETRY);
        }
    }
}

/// Notices the machine waking from sleep. QUIC times connections by the
/// monotonic clock, which stops during sleep on macOS and Linux, so a
/// connection the peer dropped meanwhile looks alive for its whole idle
/// timeout after wake. Cutting them dials every peer again at once.
async fn watch_sleep(inner: Arc<Inner>) {
    let mut wall = SystemTime::now();
    let mut mono = Instant::now();

    loop {
        sleep(TICK).await;
        let (wall_now, mono_now) = (SystemTime::now(), Instant::now());
        let passed = wall_now
            .duration_since(wall)
            .unwrap_or_default()
            .max(mono_now - mono);
        (wall, mono) = (wall_now, mono_now);

        if inner.is_closed() {
            return;
        }

        if passed > TICK + SLEPT {
            info!(
                slept_s = passed.as_secs(),
                "woke from sleep; dialing every peer again"
            );
            inner.woke().await;
        }
    }
}

impl Inner {
    /// Drops every connection after a sleep and has iroh probe its paths
    /// again; the dialers and the next calls connect afresh.
    pub(crate) async fn woke(&self) {
        self.endpoint.network_change().await;

        for live in self.conns().values() {
            live.conn.close(0u32.into(), b"slept");
        }

        for retired in self.retired().values() {
            retired.conn.close(0u32.into(), b"slept");
        }

        self.reset_channels();
        self.addr_wake.notify_waiters();
        self.bump();
    }
}
