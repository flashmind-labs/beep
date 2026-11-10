//! Owns peer dialers and retires connections after membership changes.

use super::{Inner, PEER_ALPN};
use crate::link;
use iroh::endpoint::Connection;
use iroh::{EndpointAddr, EndpointId};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::{AbortHandle, JoinSet};
use tokio::time::{Instant, sleep, timeout};
use tracing::{debug, warn};

const FIRST_RETRY: Duration = Duration::from_secs(1);
/// A member that stays away is tried about once a minute. One that comes
/// back dials us itself, and its new address ends our wait early.
const LAST_RETRY: Duration = Duration::from_secs(60);
const DIAL_TIMEOUT: Duration = Duration::from_secs(10);
/// A connection that lasted this long counts as having worked.
const STABLE: Duration = Duration::from_secs(5);
/// A removed peer's link may take this long to deliver the news before it is cut.
const GRACE: Duration = Duration::from_secs(3);
/// The larger id lets the smaller dial first, so one dial does the work.
const HEAD_START: Duration = Duration::from_millis(300);

/// 1 s doubling to `LAST_RETRY`, each shifted by up to a quarter.
struct Backoff(Duration);

impl Backoff {
    fn pause(&mut self) -> Duration {
        let wait = self.0.mul_f64(0.75 + fastrand::f64() / 2.0);
        self.0 = (self.0 * 2).min(LAST_RETRY);
        wait
    }

    fn reset(&mut self) {
        self.0 = FIRST_RETRY;
    }
}

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

    loop {
        rev.borrow_and_update();
        inner.prune_channels();
        let view = inner.view();
        let member = view.devices.is_member(inner.me);
        let wanted: Vec<EndpointId> = if member {
            view.devices
                .members()
                .into_iter()
                .map(|d| d.id)
                .filter(|id| *id != inner.me)
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
async fn dial(inner: Arc<Inner>, peer: EndpointId) -> EndpointId {
    let mut backoff = Backoff(FIRST_RETRY);
    let mut waited = false;

    loop {
        let notified = inner.wake.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();

        if !inner.is_member(peer) || !inner.is_member(inner.me) || inner.is_closed() {
            return peer;
        }

        if let Some(conn) = inner.live(peer) {
            tokio::select! {
                () = &mut notified => {}
                _ = conn.closed() => {}
            }
            continue;
        }

        if inner.me > peer && !waited {
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
        let started = Instant::now();
        let connect = timeout(DIAL_TIMEOUT, inner.endpoint.connect(addr, PEER_ALPN));
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
        let Some(dialed) = dialed else { continue };

        match dialed {
            Ok(Ok(conn)) => {
                link::serve(inner.clone(), conn, inner.me).await;
                waited = false;

                if started.elapsed() >= STABLE {
                    backoff.reset();
                    continue;
                }
            }
            Ok(Err(e)) => debug!("dial failed: {e}"),
            Err(_) => debug!("dial timed out"),
        }
        // Only a new address for this peer ends the wait early.
        let pause = sleep(backoff.pause());
        tokio::pin!(pause);

        loop {
            let moved = inner.addr_wake.notified();
            tokio::pin!(moved);
            moved.as_mut().enable();
            let now = inner.addrs().get(&peer).cloned();

            if now != tried {
                break;
            }

            tokio::select! {
                () = &mut pause => break,
                () = &mut moved => {}
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/mesh_owner.rs"]
mod tests;
