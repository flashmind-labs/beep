//! Serves `beep-sessions/1` to the mesh's devices.

use super::read::{Access, Asked, serve};
use crate::{agents::Agents, link::Membership};
use beep_mesh::{Key, Mesh};
use beep_proto::node::mesh_state::{SessionMove, StateValue};
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

/// Whether `ask` about session `id` is one the move record in the mesh
/// allows between this node (`me`) and `peer`: a target reads the
/// transcript of the session that moves to it, and a source asks the
/// target how the move stands. A device that no record names reads
/// nothing, whatever it is in the mesh.
pub(super) fn permits(
    record: Option<&SessionMove>,
    me: EndpointId,
    peer: EndpointId,
    ask: Asked,
    id: &str,
) -> bool {
    let Some(record) = record.filter(|record| record.id == id) else {
        return false;
    };

    match ask {
        Asked::Read => record.from == me && record.to == peer,
        Asked::State => record.to == me && record.from == peer,
    }
}

/// The move record of session `id` in `mesh`.
fn record_of(mesh: &Mesh, id: &str) -> Option<SessionMove> {
    let bytes = mesh.get(&Key::SessionMove(id.to_owned()))?;
    SessionMove::from_bytes(&bytes).ok()
}

/// What `mesh` calls this device, for a refusal a person reads.
fn name_of_me(mesh: &Mesh) -> String {
    let me = mesh.me();
    mesh.devices()
        .into_iter()
        .find(|status| status.device.id == me)
        .map_or_else(|| me.fmt_short().to_string(), |status| status.device.name)
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
                    let Some(mesh) = self.membership.get() else {
                        continue;
                    };
                    streams.spawn(async move {
                        let updates = Some(mesh.updates());
                        let allow = |ask: Asked, id: &str| {
                            mesh.is_member(peer)
                                && permits(record_of(&mesh, id).as_ref(), mesh.me(), peer, ask, id)
                        };
                        let me = name_of_me(&mesh);
                        let access = Access { check: &allow, updates };
                        serve(tx, rx, &agents, &me, access).await
                    });
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
