//! Connections for commands and subscriptions, separate from mesh replication.

use super::{Inner, Mesh};
use crate::{MeshError, OPEN_TIMEOUT, wire};
use iroh::{EndpointAddr, EndpointId, endpoint::Connection};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, PoisonError},
};
use tokio::{sync::Mutex as AsyncMutex, time::timeout};

struct CachedConnection {
    connection: Connection,
    peer_session: Option<u64>,
}

#[derive(Default)]
pub(super) struct ChannelConnection {
    connection: Mutex<Option<CachedConnection>>,
    dialing: AsyncMutex<()>,
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

impl Mesh {
    /// Tracks an accepted protocol connection for membership and restart invalidation.
    pub fn track_protocol_connection(&self, connection: &Connection) -> Result<(), MeshError> {
        let peer = connection.remote_id();
        let alpn = [
            beep_proto::node::commands::ALPN,
            beep_proto::node::subscriptions::ALPN,
        ]
        .into_iter()
        .find(|alpn| *alpn == connection.alpn())
        .ok_or_else(|| wire::net("unknown peer protocol"))?;
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

    async fn protocol_connection(
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

        let _dialing = slot.dialing.lock().await;
        if let Some(connection) = slot.live(self.inner.conns().get(&peer).map(|live| live.session))
        {
            return Ok(connection);
        }

        let opening_session = self.inner.conns().get(&peer).map(|live| live.session);
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
        let connection = match timeout(OPEN_TIMEOUT, self.inner.endpoint.connect(addr, alpn)).await
        {
            Ok(Ok(connection)) => connection,
            Ok(Err(error)) => return Err(wire::net(error)),
            Err(_) => return Err(wire::net("opening the peer protocol timed out")),
        };

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
