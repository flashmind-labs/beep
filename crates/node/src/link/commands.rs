//! Commands on their own authenticated QUIC connection.

use crate::events::HEARTBEAT;
use super::{
    Offers, Session, channels, events,
    peers::{CHANNEL_OPEN_WAIT, Peers},
};
use anyhow::{Context, Result};
use beep_proto::node::{
    LinkRx, LinkTx,
    commands::{Open, Reply, Request},
    link,
};
use iroh::{
    EndpointId,
    endpoint::{Connection, RecvStream, SendStream},
    protocol::{AcceptError, ProtocolHandler},
};
use std::{collections::HashSet, fmt, time::Duration};
use tokio::{
    task::JoinSet,
    time::{Instant, interval_at, timeout},
};

#[derive(Clone)]
pub struct CommandsAlpn(Peers);

impl CommandsAlpn {
    pub fn new(peers: &Peers) -> Self {
        Self(peers.clone())
    }
}

impl fmt::Debug for CommandsAlpn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CommandsAlpn")
    }
}

impl ProtocolHandler for CommandsAlpn {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        channels::serve_connection(&self.0, connection, serve_stream).await
    }
}

fn serve_stream(
    peers: Peers,
    offers: Offers,
    peer: EndpointId,
    mesh: EndpointId,
    send: SendStream,
    recv: RecvStream,
) -> Session {
    Box::pin(async move {
        let (tx, mut rx) = link(send, recv);
        let open = read_open(&mut rx).await?;
        anyhow::ensure!(peers.channel_allowed(peer, mesh), "not authorized");
        match open {
            Open::Calls => {
                anyhow::ensure!(
                    peers.controller_allowed(peer, mesh),
                    "only a controller may send actions"
                );
                serve(tx, rx, offers, peer).await
            }
            Open::Session { protocol } => {
                peers
                    .serve_command_session(tx, rx, peer, mesh, &protocol)
                    .await
            }
        }
    })
}

async fn read_open(rx: &mut LinkRx) -> Result<Open> {
    timeout(CHANNEL_OPEN_WAIT, rx.recv::<Open>())
        .await??
        .context("the command stream closed before opening")
}

pub(super) async fn serve_open(
    tx: LinkTx,
    mut rx: LinkRx,
    offers: Offers,
    caller: EndpointId,
) -> Result<()> {
    let open = read_open(&mut rx).await?;
    anyhow::ensure!(
        open == Open::Calls,
        "this stream requires correlated commands"
    );
    serve(tx, rx, offers, caller).await
}

async fn serve(mut tx: LinkTx, mut rx: LinkRx, offers: Offers, caller: EndpointId) -> Result<()> {
    let mut calls = JoinSet::new();
    let mut pending = HashSet::new();
    let mut heartbeat = interval_at(Instant::now() + HEARTBEAT, HEARTBEAT);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    send(&mut tx, Reply::Ready).await?;

    loop {
        tokio::select! {
            request = rx.recv::<Request>() => {
                anyhow::ensure!(offers.lends_to_peers() || offers.mesh.get().is_some_and(|mesh| mesh.me() == caller), "this machine is no longer shared");
                match request? {
                    None => return Ok(()),
                    Some(Request::Ping { id }) => send(&mut tx, Reply::Pong { id }).await?,
                    Some(Request::Call { id, operation }) => {
                        if calls.len() >= 16 || !pending.insert(id) {
                            send(&mut tx, Reply::Error { id, reason: "too many pending calls or duplicate call id".into() }).await?;
                        } else {
                            let agents = offers.agents.clone();
                            calls.spawn(async move { (id, timeout(Duration::from_secs(120), events::execute(agents, caller, *operation)).await) });
                        }
                    },
                }
            },
            Some(result) = calls.join_next(), if !calls.is_empty() => {
                let (id, result) = result.context("a command stopped")?;
                pending.remove(&id);
                let reply = match result {
                    Ok(Ok(reply)) => Reply::Result { id, reply: Box::new(reply) },
                    Ok(Err(error)) => Reply::Error { id, reason: format!("{error:#}") },
                    Err(_) => Reply::Error { id, reason: "command timed out; its outcome may be unknown".into() },
                };
                send(&mut tx, reply).await?;
            },
            _ = heartbeat.tick() => send(&mut tx, Reply::Pong { id: 0 }).await?,
        }
    }
}

async fn send(tx: &mut LinkTx, reply: Reply) -> Result<()> {
    timeout(Duration::from_secs(15), tx.send(&reply))
        .await
        .context("command reader stopped receiving")?
}
