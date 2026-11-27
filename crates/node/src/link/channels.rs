//! Authenticated command, subscription and forward connections.

use super::{Offers, Session, peers::Peers};
use iroh::{
    EndpointId,
    endpoint::{Connection, RecvStream, SendStream},
    protocol::AcceptError,
};
use tokio::task::JoinSet;
use tracing::Instrument;

const MAX_STREAMS: usize = 64;

type ServeStream = fn(Peers, Offers, EndpointId, EndpointId, SendStream, RecvStream) -> Session;

pub(super) async fn serve_connection(
    peers: &Peers,
    connection: Connection,
    serve: ServeStream,
) -> Result<(), AcceptError> {
    let peer = connection.remote_id();
    let Some((mesh, mut updates)) = peers.channel_access(&connection) else {
        connection.close(1u32.into(), b"not authorized");
        return Ok(());
    };
    let offers = peers.channel_offers();
    let mut membership = offers.mesh.subscribe();
    let mut tasks = JoinSet::new();

    loop {
        if !peers.channel_allowed(peer, mesh) {
            connection.close(1u32.into(), b"not authorized");
            break;
        }

        tokio::select! {
            _ = connection.closed() => break,
            changed = updates.changed() => if changed.is_err() { break; },
            _ = membership.changed() => {
                connection.close(1u32.into(), b"mesh changed");
                break;
            },
            accepted = connection.accept_bi() => {
                let Ok((mut send, mut recv)) = accepted else { break; };

                if tasks.len() >= MAX_STREAMS || !peers.channel_allowed(peer, mesh) {
                    send.reset(1u32.into()).ok();
                    recv.stop(1u32.into()).ok();
                    continue;
                }

                let offers = offers.clone();
                let peers = peers.clone();
                tasks.spawn(async move {
                    let result = serve(peers, offers, peer, mesh, send, recv).await;
                    if let Err(error) = result {
                        log::debug!("peer channel stream ended: {error:#}");
                    }
                }.in_current_span());
            },
            Some(ended) = tasks.join_next(), if !tasks.is_empty() => {
                if let Err(error) = ended {
                    log::warn!("peer channel stream stopped: {error}");
                }
            },
        }
    }

    Ok(())
}
