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
/// How soon a source asks its target again after it said something new;
/// each answer with nothing new doubles the wait, up to `MAX_ASK_EVERY`.
const ASK_EVERY: Duration = Duration::from_secs(2);
const MAX_ASK_EVERY: Duration = Duration::from_secs(10);
/// How long the target has to answer once reached.
const ANSWER_WITHIN: Duration = Duration::from_secs(30);

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
    let answer = timeout(ANSWER_WITHIN, rx.recv::<Reply>())
        .await
        .context("the target did not answer in time")??;
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
/// A target out of reach, or one from before moves, says nothing new. Each
/// change, reaching it or not, is logged once.
pub(super) async fn watch_target(env: &Env, record: &SessionMove, said: watch::Sender<Target>) {
    let mut wait = ASK_EVERY;
    // `Some(true)` once the last ask reached the target, `Some(false)` once
    // it did not.
    let mut reached = None;

    loop {
        let more = match ask(env, record).await {
            Ok(now) => {
                if reached != Some(true) {
                    log::info!(
                        "session {}: asked the target how the move stands",
                        record.id
                    );
                    reached = Some(true);
                }

                said.send_if_modified(|before| {
                    // What was told stays told, unless the target says more.
                    let more = now != Target::Unknown && *before != now;

                    if more {
                        log::info!("session {}: the target says {now:?}", record.id);
                        *before = now;
                    }

                    more
                })
            }
            Err(e) => {
                if reached != Some(false) {
                    log::warn!("session {}: cannot ask the target: {e:#}", record.id);
                    reached = Some(false);
                } else {
                    log::debug!("session {} target not asked: {e:#}", record.id);
                }

                false
            }
        };
        wait = if more {
            ASK_EVERY
        } else {
            (wait * 2).min(MAX_ASK_EVERY)
        };
        sleep(wait).await;
    }
}
