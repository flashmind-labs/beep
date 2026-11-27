//! Serves `beep-sessions/1` to the mesh's devices.

use super::{
    device_name,
    folder::project_folder,
    progress::Moves,
    read::{Access, Asked, Leaving, serve},
    saved::Outgoing,
};
use crate::{agents::conversation, membership::Membership};
use beep_mesh::{Key, Mesh};
use beep_proto::SessionId;
use beep_proto::node::mesh_state::{SessionMove, StateValue};
use iroh::{
    EndpointId,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler},
};
use std::{fmt, path::PathBuf, sync::Arc};
use tokio::task::JoinSet;
use tracing::Instrument;

/// The most reads one peer has open at once.
const MAX_STREAMS: usize = 4;

/// Answers other devices' reads of the sessions this node keeps.
#[derive(Clone)]
pub struct SessionsAlpn {
    moves: Arc<Moves>,
    membership: Membership,
}

impl SessionsAlpn {
    pub(crate) fn new(moves: Arc<Moves>, membership: Membership) -> Self {
        Self { moves, membership }
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
    id: &SessionId,
) -> bool {
    let Some(record) = record.filter(|record| record.id == *id) else {
        return false;
    };

    match ask {
        Asked::Read => record.from == me && record.to == peer,
        Asked::State(at) => record.at == at && record.to == me && record.from == peer,
    }
}

/// What this machine keeps of the session `record` moves away: its
/// project's folder, or for a chat's session the folder under this
/// machine's own chats, whatever the record says of where that is.
fn leaving(mesh: &Mesh, record: &SessionMove) -> Option<Leaving> {
    if record.conversation.is_none() {
        let folder = project_folder(mesh, &record.project, &mesh.me())?;
        return Some(Leaving::Project {
            folder: Into::<PathBuf>::into(folder),
            worktrees: record.worktrees.clone(),
        });
    }

    let home = dirs::home_dir()?;
    let folder = conversation::moved(&home, &record.path)?;
    let apart = chat_moves(mesh)
        .filter(|other| other.from == record.from && other.id != record.id)
        .filter_map(|other| conversation::moved(&home, &other.path))
        .filter(|other| other.starts_with(&folder))
        .collect();
    Some(Leaving::Chat { folder, apart })
}

/// The records in `mesh` that move a chat's session.
fn chat_moves(mesh: &Mesh) -> impl Iterator<Item = SessionMove> {
    mesh.scan("session-move/")
        .into_iter()
        .filter_map(|(_, bytes)| SessionMove::from_bytes(&bytes).ok())
        .filter(|record| record.conversation.is_some())
}

/// The move record of session `id` in `mesh`.
fn record_of(mesh: &Mesh, id: &SessionId) -> Option<SessionMove> {
    let bytes = mesh.get(&Key::SessionMove(id.clone()))?;
    SessionMove::from_bytes(&bytes).ok()
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
                    let moves = self.moves.clone();
                    let Some(mesh) = self.membership.get() else {
                        continue;
                    };
                    streams.spawn(async move {
                        let updates = Some(mesh.updates());
                        let allow = |ask: Asked, id: &SessionId| {
                            mesh.is_member(peer)
                                && (permits(record_of(&mesh, id).as_ref(), mesh.me(), peer, ask, id)
                                    || matches!(ask, Asked::State(at) if moves.owns_target(id, at, peer)))
                        };
                        let leaving = |id: &SessionId| leaving(&mesh, &record_of(&mesh, id)?);
                        let me = device_name(&mesh, &mesh.me());
                        let outgoing = |id: &SessionId| {
                            let record = record_of(&mesh, id)?;
                            (mesh.is_member(peer) && permits(Some(&record), mesh.me(), peer, Asked::Read, id))
                                .then_some(Outgoing { at: record.at, to: record.to })
                        };
                        let access = Access { check: &allow, updates, leaving: &leaving, outgoing: Some(&outgoing) };
                        serve(tx, rx, &moves, &me, access).await
                    }.in_current_span());
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
