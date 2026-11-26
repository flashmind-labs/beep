//! Connections for commands, subscriptions and port forwards, separate from
//! mesh replication: the slots one mesh keeps for them.

use super::Inner;
use iroh::{EndpointId, endpoint::Connection};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};
use tokio::{sync::Mutex as AsyncMutex, time::Instant};

pub(crate) struct FailedDial {
    pub peer_session: Option<u64>,
    pub retry_at: Instant,
    pub reason: String,
}

pub(crate) struct CachedConnection {
    pub connection: Connection,
    pub peer_session: Option<u64>,
}

#[derive(Default)]
pub(crate) struct ChannelConnection {
    pub connection: Mutex<Option<CachedConnection>>,
    pub dialing: AsyncMutex<Option<FailedDial>>,
    pub incoming: Mutex<Vec<CachedConnection>>,
}

impl ChannelConnection {
    pub(crate) fn live(&self, peer_session: Option<u64>) -> Option<Connection> {
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
    pub(crate) fn channel_connections(&self) -> Vec<Connection> {
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
    pub(crate) fn reset_channels(&self) {
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

    pub(crate) fn prune_channels(&self) {
        let conns = self.conns();
        self.channels
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|(peer, _), slot| {
                // A restarted peer's new mesh hello invalidates its old channels too.
                slot.live(conns.get(peer).map(|live| live.session));
                let keep = self.is_member(self.me()) && self.is_member(*peer);
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

pub(crate) type Channels = Mutex<BTreeMap<(EndpointId, &'static [u8]), Arc<ChannelConnection>>>;
