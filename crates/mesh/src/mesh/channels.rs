//! Connections for commands and subscriptions, separate from mesh replication.

use super::{Inner, Mesh};
use crate::{MeshError, OPEN_TIMEOUT, wire};
use iroh::{EndpointAddr, EndpointId, endpoint::Connection};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, PoisonError},
};
use tokio::{
    sync::Mutex as AsyncMutex,
    time::{Duration, Instant, timeout},
};

const FIRST_RETRY: Duration = Duration::from_secs(1);
const MAX_RETRY: Duration = Duration::from_secs(30);

struct FailedDial {
    peer_session: Option<u64>,
    retry_at: Instant,
    pause: Duration,
    reason: String,
}

struct CachedConnection {
    connection: Connection,
    peer_session: Option<u64>,
}

#[derive(Default)]
pub(super) struct ChannelConnection {
    connection: Mutex<Option<CachedConnection>>,
    dialing: AsyncMutex<Option<FailedDial>>,
    incoming: Mutex<Vec<CachedConnection>>,
}

impl ChannelConnection {
    fn live(&self, peer_session: Option<u64>) -> Option<Connection> {
        self.incoming
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|cached| {
                if peer_session.is_some() && cached.peer_session != peer_session {
                    cached.connection.close(3u32.into(), b"peer restarted");
                }
                cached.connection.close_reason().is_none()
            });
        let mut cached = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let current = cached.as_mut()?;
        if peer_session.is_some() && current.peer_session != peer_session {
            current.connection.close(3u32.into(), b"peer restarted");
            *cached = None;
            return None;
        }

        (current.connection.close_reason().is_none()).then(|| current.connection.clone())
    }
}

impl Inner {
    pub(super) fn channel_connections(&self) -> Vec<Connection> {
        self.channels
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .flat_map(|slot| {
                let outgoing = slot.live(None);
                let incoming = slot.incoming.lock().unwrap_or_else(PoisonError::into_inner);
                outgoing
                    .into_iter()
                    .chain(incoming.iter().map(|cached| cached.connection.clone()))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    pub(crate) fn restart_connections(&self, peer: EndpointId, session: u64) {
        for retired in self.retired().values() {
            if retired.conn.remote_id() == peer && retired.session != session {
                retired.conn.close(3u32.into(), b"peer restarted");
            }
        }

        for ((id, _), slot) in self
            .channels
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
        {
            if *id == peer {
                slot.live(Some(session));
            }
        }
    }

    /// Closes every command and subscription connection and forgets failed
    /// dials, so the next call dials again at once.
    pub(super) fn reset_channels(&self) {
        for slot in self
            .channels
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
        {
            if let Some(cached) = slot
                .connection
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
            {
                cached.connection.close(0u32.into(), b"slept");
            }

            for cached in slot
                .incoming
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .drain(..)
            {
                cached.connection.close(0u32.into(), b"slept");
            }

            // A dial in progress holds the lock and records its own outcome.
            if let Ok(mut failed) = slot.dialing.try_lock() {
                *failed = None;
            }
        }
    }

    pub(super) fn prune_channels(&self) {
        let conns = self.conns();
        self.channels
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|(peer, _), slot| {
                // A restarted peer's new mesh hello invalidates its old channels too.
                slot.live(conns.get(peer).map(|live| live.session));
                let keep = self.is_member(self.me) && self.is_member(*peer);
                if !keep {
                    if let Some(connection) = slot.live(None) {
                        connection.close(2u32.into(), b"removed");
                    }
                    for cached in slot
                        .incoming
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .drain(..)
                    {
                        cached.connection.close(2u32.into(), b"removed");
                    }
                }
                keep
            });
    }
}

pub(super) type Channels = Mutex<BTreeMap<(EndpointId, &'static [u8]), Arc<ChannelConnection>>>;

/// The command or subscription protocol `connection` speaks, if either.
fn protocol_alpn(connection: &Connection) -> Option<&'static [u8]> {
    [
        beep_proto::node::commands::ALPN,
        beep_proto::node::subscriptions::ALPN,
    ]
    .into_iter()
    .find(|alpn| *alpn == connection.alpn())
}

impl Mesh {
    /// Tracks an accepted protocol connection for membership and restart invalidation.
    pub fn track_protocol_connection(&self, connection: &Connection) -> Result<(), MeshError> {
        let peer = connection.remote_id();
        let alpn = protocol_alpn(connection).ok_or_else(|| wire::net("unknown peer protocol"))?;
        let conns = self.inner.conns();
        let peer_session = conns.get(&peer).map(|live| live.session);
        let mut channels = self
            .inner
            .channels
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.check_channel_member(peer)?;
        let slot = channels.entry((peer, alpn)).or_default();
        slot.live(peer_session);
        slot.incoming
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(CachedConnection {
                connection: connection.clone(),
                peer_session,
            });
        Ok(())
    }

    pub async fn command_connection(&self, peer: EndpointId) -> Result<Connection, MeshError> {
        self.protocol_connection(peer, beep_proto::node::commands::ALPN)
            .await
    }

    pub async fn subscription_connection(&self, peer: EndpointId) -> Result<Connection, MeshError> {
        self.protocol_connection(peer, beep_proto::node::subscriptions::ALPN)
            .await
    }

    pub(crate) async fn protocol_connection(
        &self,
        peer: EndpointId,
        alpn: &'static [u8],
    ) -> Result<Connection, MeshError> {
        self.check_channel_member(peer)?;
        let slot = self
            .inner
            .channels
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry((peer, alpn))
            .or_default()
            .clone();

        if let Some(connection) = slot.live(self.inner.conns().get(&peer).map(|live| live.session))
        {
            return Ok(connection);
        }

        let mut failed = slot.dialing.lock().await;
        if let Some(connection) = slot.live(self.inner.conns().get(&peer).map(|live| live.session))
        {
            return Ok(connection);
        }

        let opening_session = self.inner.conns().get(&peer).map(|live| live.session);
        if let Some(previous) = failed.as_ref()
            && previous.peer_session == opening_session
            && Instant::now() < previous.retry_at
        {
            return Err(wire::net(&previous.reason));
        }

        let active = self.inner.live(peer);
        let paths = active
            .as_ref()
            .map(|connection| {
                connection
                    .paths()
                    .iter()
                    .map(|path| path.remote_addr().clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let addr = if paths.is_empty() {
            self.inner
                .addrs()
                .get(&peer)
                .map(|note| note.addr.clone())
                .unwrap_or_else(|| EndpointAddr::new(peer))
        } else {
            EndpointAddr::new(peer).with_addrs(paths)
        };
        let result = timeout(OPEN_TIMEOUT, self.inner.endpoint.connect(addr, alpn))
            .await
            .map_err(|_| "opening the peer protocol timed out".to_owned())
            .and_then(|result| result.map_err(|error| error.to_string()));
        let connection = match result {
            Ok(connection) => connection,
            Err(reason) => {
                let pause = failed
                    .as_ref()
                    .filter(|previous| previous.peer_session == opening_session)
                    .map_or(FIRST_RETRY, |previous| (previous.pause * 2).min(MAX_RETRY));
                tracing::warn!(?peer, protocol = %String::from_utf8_lossy(alpn), retry_ms = pause.as_millis(), %reason, "peer protocol connection failed; backing off");
                *failed = Some(FailedDial {
                    peer_session: opening_session,
                    retry_at: Instant::now() + pause,
                    pause,
                    reason: reason.clone(),
                });

                return Err(wire::net(reason));
            }
        };
        *failed = None;

        // Serialize the final membership check with pruning, so a removal
        // cannot miss a connection that has just finished dialing.
        let conns = self.inner.conns();
        let peer_session = conns.get(&peer).map(|live| live.session);
        if opening_session != peer_session {
            connection.close(3u32.into(), b"peer restarted");
            return Err(wire::net("the peer restarted while opening its protocol"));
        }
        let _channels = self
            .inner
            .channels
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Err(error) = self.check_channel_member(peer) {
            connection.close(2u32.into(), b"removed");
            return Err(error);
        }

        *slot
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(CachedConnection {
            connection: connection.clone(),
            peer_session,
        });
        Ok(connection)
    }

    /// Closes a protocol connection that is still open but took no new
    /// stream, so the next one dials again instead of reusing it until the
    /// app restarts. A node that is full resets new streams, and a link
    /// that went quiet after a network change or a suspended app never
    /// answers them; either way the mesh link may still be up.
    pub(crate) fn drop_protocol_connection(&self, connection: &Connection) {
        let slot = protocol_alpn(connection).and_then(|alpn| {
            self.inner
                .channels
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(&(connection.remote_id(), alpn))
                .cloned()
        });

        if let Some(slot) = slot {
            let mut cached = slot
                .connection
                .lock()
                .unwrap_or_else(PoisonError::into_inner);

            if cached
                .as_ref()
                .is_some_and(|held| held.connection.stable_id() == connection.stable_id())
            {
                *cached = None;
            }
        }

        if connection.close_reason().is_none() {
            tracing::warn!(peer = ?connection.remote_id(), protocol = %String::from_utf8_lossy(connection.alpn()), "a protocol connection took no new stream; dialing again");
            connection.close(4u32.into(), b"stalled");
        }
    }

    fn check_channel_member(&self, peer: EndpointId) -> Result<(), MeshError> {
        if self.is_member(self.me()) && self.is_member(peer) && !self.inner.is_closed() {
            Ok(())
        } else {
            Err(wire::net(
                "the peer protocol requires current mesh membership",
            ))
        }
    }
}

#[cfg(test)]
#[path = "../tests/protocol_connections.rs"]
mod tests;
