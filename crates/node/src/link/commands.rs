//! Commands on their own authenticated QUIC connection.

use super::{Offers, events, peers::CHANNEL_OPEN_WAIT};
use crate::events::HEARTBEAT;
use anyhow::{Context, Result};
use beep_proto::node::{
    LinkRx, LinkTx,
    commands::{Open, Reply, Request},
};
use iroh::EndpointId;
use std::{collections::HashSet, time::Duration};
use tokio::{
    task::JoinSet,
    time::{Instant, interval_at, timeout},
};

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
