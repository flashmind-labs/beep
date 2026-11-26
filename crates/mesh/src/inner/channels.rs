//! Connections for commands, subscriptions and port forwards, separate from
//! mesh replication: the slots one mesh keeps for them.

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

pub(crate) type Channels = Mutex<BTreeMap<(EndpointId, &'static [u8]), Arc<ChannelConnection>>>;
