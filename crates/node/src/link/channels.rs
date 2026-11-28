//! Authenticated command, subscription and forward connections, and the
//! node's ALPNs on a router.

use super::{Offers, commands, forward, peers::Peers, streams, subscriptions};
use anyhow::Result;
use beep_mesh::{JOIN_ALPN, PEER_ALPN};
use beep_proto::node::{commands::Open, link};
use iroh::{
    EndpointId,
    endpoint::{Connection, RecvStream, SendStream},
    protocol::{AcceptError, ProtocolHandler, RouterBuilder},
};
use tokio::task::JoinSet;
use tracing::Instrument;

const MAX_STREAMS: usize = 64;

/// Which protocol a channel's connection carries.
#[derive(Clone, Copy, Debug)]
enum Channel {
    Commands,
    Subscriptions,
    Forward,
}

/// Serves `beep-commands`, `beep-subscriptions` and `beep-forward`: one
/// authenticated connection per channel, a stream per call.
#[derive(Clone, Debug)]
pub struct ChannelAlpn {
    peers: Peers,
    channel: Channel,
}

impl ProtocolHandler for ChannelAlpn {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        serve_connection(&self.peers, connection, self.channel).await
    }
}

/// Adds every ALPN the node serves to `router`: the mesh's, the join's, the
/// session reads of a move, the three channels and the team's.
pub fn mount(peers: &Peers, router: RouterBuilder) -> RouterBuilder {
    let channel = |channel| ChannelAlpn {
        peers: peers.clone(),
        channel,
    };

    router
        .accept(PEER_ALPN, peers.peer_handler())
        .accept(JOIN_ALPN, peers.join_handler())
        .accept(beep_proto::node::commands::ALPN, channel(Channel::Commands))
        .accept(
            beep_proto::node::subscriptions::ALPN,
            channel(Channel::Subscriptions),
        )
        .accept(beep_proto::node::forward::ALPN, channel(Channel::Forward))
        .accept(crate::agents::SESSIONS_ALPN, peers.sessions_handler())
        .accept(beep_proto::team::ALPN, peers.team_handler())
}

async fn serve_connection(
    peers: &Peers,
    connection: Connection,
    channel: Channel,
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
                    let result = serve_stream(channel, peers, offers, peer, mesh, send, recv).await;
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

/// Serves one stream of a channel, once the peer is known to be allowed.
async fn serve_stream(
    channel: Channel,
    peers: Peers,
    offers: Offers,
    peer: EndpointId,
    mesh: EndpointId,
    send: SendStream,
    recv: RecvStream,
) -> Result<()> {
    match channel {
        Channel::Commands => serve_commands(peers, offers, peer, mesh, send, recv).await,
        Channel::Subscriptions => {
            subscriptions::serve_stream(peers, offers, peer, mesh, send, recv).await
        }
        Channel::Forward => forward::serve_stream(peers, peer, mesh, send, recv).await,
    }
}

/// A command stream carries either calls or a session stream.
async fn serve_commands(
    peers: Peers,
    offers: Offers,
    peer: EndpointId,
    mesh: EndpointId,
    send: SendStream,
    recv: RecvStream,
) -> Result<()> {
    let (tx, mut rx) = link(send, recv);
    let open = commands::read_open(&mut rx).await?;
    anyhow::ensure!(peers.channel_allowed(peer, mesh), "not authorized");

    match open {
        Open::Calls => {
            anyhow::ensure!(
                peers.controller_allowed(peer, mesh),
                "only a controller may send actions"
            );
            commands::serve(tx, rx, offers, peer).await
        }
        Open::Session { protocol, compress } => {
            streams::serve_channel_session(&peers, tx, rx, peer, mesh, &protocol, compress).await
        }
    }
}
