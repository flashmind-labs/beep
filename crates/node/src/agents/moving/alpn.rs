//! Serves `beep-sessions/1` to the mesh's devices.

use super::read::serve;
use crate::{agents::Agents, link::Membership};
use iroh::{
    EndpointId,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler},
};
use std::{fmt, sync::Arc};
use tokio::task::JoinSet;

/// The most reads one peer has open at once.
const MAX_STREAMS: usize = 4;

/// Answers other devices' reads of the sessions this node keeps.
#[derive(Clone)]
pub struct SessionsAlpn {
    agents: Arc<Agents>,
    membership: Membership,
}

impl SessionsAlpn {
    pub(crate) fn new(agents: Arc<Agents>, membership: Membership) -> Self {
        Self { agents, membership }
    }

    fn allows(&self, peer: &EndpointId) -> bool {
        self.membership
            .get()
            .is_some_and(|mesh| mesh.is_member(*peer))
    }
}

impl fmt::Debug for SessionsAlpn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionsAlpn")
    }
}

impl ProtocolHandler for SessionsAlpn {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let peer = connection.remote_id();
        if !self.allows(&peer) {
            log::info!("refused a session read from {peer}: not in the mesh");
            connection.close(1u32.into(), b"not in the mesh");
            return Ok(());
        }
        // The streams end with the connection: dropping the set stops them.
        let mut streams = JoinSet::new();
        loop {
            tokio::select! {
                stream = connection.accept_bi() => {
                    let Ok((send, recv)) = stream else {
                        return Ok(());
                    };
                    if streams.len() >= MAX_STREAMS {
                        log::warn!("refused a session read from {peer}: {MAX_STREAMS} open");
                        continue;
                    }
                    let (tx, rx) = beep_proto::node::link(send, recv);
                    let agents = self.agents.clone();
                    streams.spawn(async move { serve(tx, rx, &agents).await });
                }
                Some(ended) = streams.join_next() => {
                    match ended {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => log::debug!("a session read from {peer} ended: {e:#}"),
                        Err(e) => log::warn!("a session read task failed: {e}"),
                    }
                    if !self.allows(&peer) {
                        connection.close(1u32.into(), b"not in the mesh");
                        return Ok(());
                    }
                }
            }
        }
    }
}
