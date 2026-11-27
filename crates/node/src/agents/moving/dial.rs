//! Reaching the other machine of a move over `beep-sessions/1`: the target
//! reads the transcript, the source asks the target how the move stands.

use super::{
    Env,
    progress::Target,
    read::{ALPN, Reply, Request},
};
use anyhow::{Context, Result, bail};
use beep_proto::SessionId;
use beep_proto::node::{LinkRx, LinkTx, link, mesh_state::SessionMove};
use iroh::{EndpointAddr, EndpointId, endpoint::Connection};
use std::time::Duration;
use tokio::{
    sync::watch,
    time::{sleep, timeout},
};

/// How long to wait for a connection.
const CONNECT_WITHIN: Duration = Duration::from_secs(15);
/// How soon a source follows a target it lost again.
const RECONNECT: Duration = Duration::from_secs(5);
/// How soon a source asks a target from before `Request::Follow` again after
/// it said something new; each answer with nothing new doubles the wait, up
/// to `MAX_ASK_EVERY`.
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

/// Runs `call` on a single `beep-sessions/1` stream to `peer`, then closes the connection.
pub(super) async fn with_stream<T, F: Future<Output = Result<T>>>(
    env: &Env,
    peer: EndpointId,
    call: impl FnOnce(LinkTx, LinkRx) -> F,
) -> Result<T> {
    let connection = dial(env, peer).await?;
    let (send, recv) = connection.open_bi().await?;
    let (tx, rx) = link(send, recv);
    let out = call(tx, rx).await;
    connection.close(0u32.into(), b"done");
    out
}

/// What the target `to` says of the move of session `id` made at `at`.
/// Asked once: for a target from before `Request::Follow`.
pub(super) async fn ask(env: &Env, to: EndpointId, id: &SessionId, at: u64) -> Result<Target> {
    let answer = with_stream(env, to, |mut tx, mut rx| async move {
        tx.send(&Request::State {
            id: id.to_owned(),
            at,
        })
        .await?;
        timeout(ANSWER_WITHIN, rx.recv::<Reply>())
            .await
            .context("the target did not answer in time")?
    })
    .await?;

    match answer {
        Some(reply @ Reply::State { .. }) => Ok(target_of(&reply)),
        Some(Reply::Refused { message }) => bail!("the target refused: {message}"),
        _ => bail!("the target ended without answering"),
    }
}

fn target_of(state: &Reply) -> Target {
    match state {
        Reply::State {
            failed: Some(reason),
            ..
        } => Target::Failed(reason.clone()),
        Reply::State { landed: true, .. } => Target::Landed,
        Reply::State { starting: true, .. } => Target::Starting,
        Reply::State { taken: true, .. } => Target::Taken,
        _ => Target::Unknown,
    }
}

/// How a followed stream ended.
enum Followed {
    /// The target landed the session or gave the move up.
    Over,
    /// The target closed the stream with the move still going, such as
    /// before it took the move up.
    Closed,
    /// The target closed it without a word: it is from before `Follow`.
    Old,
}

/// Hears each state of the move the target `to` pushes, until it landed or
/// gave the move up.
///
/// A connection that stays open before the target has reached this machine
/// stops the target from reaching it after a restart, so with `standing`
/// unset the first state is all it waits for.
async fn follow(
    env: &Env,
    to: EndpointId,
    id: &SessionId,
    at: u64,
    standing: bool,
    mut told: impl FnMut(Target),
) -> Result<Followed> {
    let connection = dial(env, to).await?;
    let followed = async {
        let (send, recv) = connection.open_bi().await?;
        let (mut tx, mut rx) = link(send, recv);
        tx.send(&Request::Follow {
            id: id.to_owned(),
            at,
        })
        .await?;
        let mut heard = false;

        loop {
            match rx.recv::<Reply>().await {
                Ok(Some(reply @ Reply::State { .. })) => {
                    heard = true;
                    let now = target_of(&reply);
                    let over = matches!(now, Target::Landed | Target::Failed(_));
                    told(now);

                    if over {
                        return Ok(Followed::Over);
                    }

                    if !standing {
                        return Ok(Followed::Closed);
                    }
                }
                Ok(Some(Reply::Refused { message })) => bail!("the target refused: {message}"),
                Ok(Some(_)) => bail!("the target answered something else"),
                Ok(None) if heard => return Ok(Followed::Closed),
                Ok(None) => return Ok(Followed::Old),
                // A stream the target reset while the connection stands: it
                // could not read the request.
                Err(_) if !heard && connection.close_reason().is_none() => {
                    return Ok(Followed::Old);
                }
                Err(e) => return Err(e),
            }
        }
    }
    .await;
    connection.close(0u32.into(), b"done");
    followed
}

/// Keeps `said` at what the target of `record` says, for as long as it runs.
/// A target out of reach says nothing new. Each change, reaching it or not,
/// is logged once.
pub(super) async fn watch_target(env: &Env, record: &SessionMove, said: watch::Sender<Target>) {
    let mut reached = None;
    let mut old = false;
    let moves = &env.moves;
    let mut contact = moves.reached();

    while !old {
        // Once the target read from this machine, it stays followed.
        let standing = moves.contacted(&record.id);
        let followed = follow(env, record.to, &record.id, record.at, standing, |now| {
            tell(&said, record, now);
        })
        .await;

        match followed {
            Ok(Followed::Over) => {
                // The source ends on what it was told.
                std::future::pending::<()>().await;
            }
            Ok(Followed::Closed) => note_reached(record, &mut reached, true, None),
            Ok(Followed::Old) => old = true,
            Err(e) => note_reached(record, &mut reached, false, Some(&e)),
        }

        if !old {
            // Until the target reached this machine, it is looked at again
            // as it does.
            tokio::select! {
                () = sleep(RECONNECT) => {}
                _ = contact.changed() => {}
            }
        }
    }

    poll_target(env, record, said).await;
}

/// What a target from before `Request::Follow` says, asked again and again.
async fn poll_target(env: &Env, record: &SessionMove, said: watch::Sender<Target>) {
    let mut wait = ASK_EVERY;
    let mut reached = None;

    loop {
        let more = match ask(env, record.to, &record.id, record.at).await {
            Ok(now) => {
                note_reached(record, &mut reached, true, None);
                tell(&said, record, now)
            }
            Err(e) => {
                note_reached(record, &mut reached, false, Some(&e));
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

/// What was told stays told, unless the target says more.
fn tell(said: &watch::Sender<Target>, record: &SessionMove, now: Target) -> bool {
    said.send_if_modified(|before| {
        let more = now != Target::Unknown && *before != now;

        if more {
            log::info!("session {}: the target says {now:?}", record.id);
            *before = now;
        }

        more
    })
}

/// Logs once each time reaching the target changes.
fn note_reached(
    record: &SessionMove,
    reached: &mut Option<bool>,
    now: bool,
    error: Option<&anyhow::Error>,
) {
    if *reached == Some(now) {
        if let Some(e) = error {
            log::debug!("session {} target not asked: {e:#}", record.id);
        }

        return;
    }

    *reached = Some(now);

    match error {
        None => log::info!(
            "session {}: asked the target how the move stands",
            record.id
        ),
        Some(e) => log::warn!("session {}: cannot ask the target: {e:#}", record.id),
    }
}

/// How the move made at `at` ended on the target `to`, once it did.
pub(super) async fn outcome(env: &Env, to: EndpointId, id: &SessionId, at: u64) -> Result<Target> {
    let mut ended = None;
    let followed = follow(env, to, id, at, true, |now| {
        if matches!(now, Target::Landed | Target::Failed(_)) {
            ended = Some(now);
        }
    })
    .await?;

    match (followed, ended) {
        (_, Some(ended)) => Ok(ended),
        (Followed::Old, None) => ask(env, to, id, at).await,
        _ => bail!("the target has not ended the move"),
    }
}
