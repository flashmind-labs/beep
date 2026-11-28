//! Commands on their own authenticated QUIC connection.

use super::{
    Offers,
    events::{Calls, WRITE_WAIT},
    peers::CHANNEL_OPEN_WAIT,
};
use crate::events::HEARTBEAT;
use anyhow::{Context, Result};
use beep_proto::node::{
    LinkRx, LinkTx,
    commands::{Open, Reply, Request},
};
use iroh::EndpointId;
use tokio::time::{Instant, interval_at, timeout};

pub(super) async fn read_open(rx: &mut LinkRx) -> Result<Open> {
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

pub(super) async fn serve(
    mut tx: LinkTx,
    mut rx: LinkRx,
    offers: Offers,
    caller: EndpointId,
) -> Result<()> {
    let mut calls = Calls::default();
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
                        if let Err(reason) = calls.start(offers.agents.clone(), caller, id, *operation) {
                            send(&mut tx, Reply::Error { id, reason: reason.into() }).await?;
                        }
                    },
                }
            },
            result = calls.finished(), if !calls.is_empty() => {
                let reply = match result? {
                    (id, Ok(reply)) => Reply::Result { id, reply: Box::new(reply) },
                    (id, Err(reason)) => Reply::Error { id, reason },
                };
                send(&mut tx, reply).await?;
            },
            _ = heartbeat.tick() => send(&mut tx, Reply::Pong { id: 0 }).await?,
        }
    }
}

async fn send(tx: &mut LinkTx, reply: Reply) -> Result<()> {
    timeout(WRITE_WAIT, tx.send(&reply))
        .await
        .context("command reader stopped receiving")?
}
