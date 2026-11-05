//! Reaching the other machine of a move over `beep-sessions/1`: the target
//! reads the transcript, the source asks the target how the move stands.

use super::{
    Env,
    progress::Target,
    read::{ALPN, Reply, Request},
};
use anyhow::{Context, Result, bail};
use beep_proto::node::{link, mesh_state::SessionMove};
use iroh::{EndpointAddr, EndpointId, endpoint::Connection};
use std::time::Duration;
use tokio::{
    sync::watch,
    time::{sleep, timeout},
};

/// How long to wait for a connection.
const CONNECT_WITHIN: Duration = Duration::from_secs(15);
/// How often a source asks its target.
const ASK_EVERY: Duration = Duration::from_secs(2);

/// A connection to `peer` for `beep-sessions/1`.
pub(super) async fn dial(env: &Env, peer: EndpointId) -> Result<Connection> {
    let addr = env
        .mesh
        .devices()
        .into_iter()
        .find(|status| status.device.id == peer)
        .and_then(|status| status.addr)
        .unwrap_or_else(|| EndpointAddr::new(peer));
    Ok(timeout(CONNECT_WITHIN, env.endpoint.connect(addr, ALPN))
        .await
        .context("could not reach the other machine in time")??)
}

/// What the target of `record` says of the move now.
async fn ask(env: &Env, record: &SessionMove) -> Result<Target> {
    let connection = dial(env, record.to).await?;
    let (send, recv) = connection.open_bi().await?;
    let (mut tx, mut rx) = link(send, recv);
    tx.send(&Request::State {
        id: record.id.clone(),
        at: record.at,
    })
    .await?;
    let answer = rx.recv::<Reply>().await?;
    connection.close(0u32.into(), b"done");
    match answer {
        Some(Reply::State {
            failed: Some(reason),
            ..
        }) => Ok(Target::Failed(reason)),
        Some(Reply::State { landed: true, .. }) => Ok(Target::Landed),
        Some(Reply::State { taken: true, .. }) => Ok(Target::Taken),
        Some(Reply::State { .. }) => Ok(Target::Unknown),
        Some(Reply::Refused { message }) => bail!("the target refused: {message}"),
        _ => bail!("the target ended without answering"),
    }
}

/// Keeps `said` at what the target of `record` says, for as long as it runs.
/// A target out of reach, or one from before moves, says nothing new.
pub(super) async fn watch_target(env: &Env, record: &SessionMove, said: watch::Sender<Target>) {
    loop {
        match ask(env, record).await {
            Ok(now) => {
                said.send_if_modified(|before| {
                    // What was told stays told, unless the target says more.
                    let more = now != Target::Unknown && *before != now;
                    if more {
                        *before = now;
                    }
                    more
                });
            }
            Err(e) => log::debug!("session {} target not asked: {e:#}", record.id),
        }
        sleep(ASK_EVERY).await;
    }
}
