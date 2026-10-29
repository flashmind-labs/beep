//! The connection to the server: the link code that joins the account, then
//! the control stream, kept open and reopened when it drops.

use crate::{agents::Agents, env, files::Folders, shell, state::State};
use anyhow::{Context, Result, bail};
use beep_proto::{
    link::LinkOffer,
    node::{Down, EnvReport, LinkRx, LinkTx, NODE_ALPN, NodeHello, Open, Opened, Up, agent, link},
};
use iroh::{
    Endpoint, EndpointAddr,
    endpoint::{Connection, RecvStream, SendStream, presets},
};
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::{
    task::JoinSet,
    time::{interval, sleep, timeout},
};
use tokio_util::task::AbortOnDropHandle;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const FIRST_RETRY: Duration = Duration::from_secs(1);
const LAST_RETRY: Duration = Duration::from_secs(60);
/// How often the machine is probed again for changes.
const REPROBE: Duration = Duration::from_secs(10 * 60);
/// How long the server has to name a session's protocol.
const OPEN_TIMEOUT: Duration = Duration::from_secs(10);
/// The most sessions open at once; more are refused.
const MAX_SESSIONS: usize = 32;

/// A session protocol a host serves beside the node's own, such as the Mac
/// app's `desktop/1`.
pub trait Protocol: Send + Sync {
    /// The name a session opens with, which the hello offers.
    fn name(&self) -> &str;
    /// Serves one session stream until it ends.
    fn serve(&self, tx: LinkTx, rx: LinkRx) -> Session;
}

/// One session served by a `Protocol`.
pub type Session = Pin<Box<dyn Future<Output = Result<()>> + Send>>;

pub fn print_offer(state: &State) -> Result<()> {
    let offer = LinkOffer::sign(&state.key, state.server, &state.name, SystemTime::now())?;
    println!("Link this machine from Beep on your phone: copy the link below to the phone, then");
    println!("open Settings, Linked devices, Type a code instead, and paste it. Check that the");
    println!("phone shows the same code.");
    println!();
    println!("  Code: {}", offer.code());
    println!("  Link: {}", offer.to_uri());
    println!();
    println!("The code works for 10 minutes. Then start it with `beep-node run`.");
    Ok(())
}

/// Stays connected until the process stops, waiting longer after each failed try.
pub async fn serve(state: State) -> Result<()> {
    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(state.key.clone())
        .bind()
        .await?;
    log::info!("node key {}", endpoint.id());
    // Made once, so agent sessions outlive each connection.
    let mut offers = Offers {
        agents: Arc::new(Agents::new(crate::agents::detect(&state.agents))),
        extra: Vec::new(),
    };
    // Files only when the owner shared folders in node.toml.
    let folders = Folders::new(&state.files);
    if !folders.is_empty() {
        offers.extra.push(Arc::new(folders));
    }
    log::info!("offering {:?}", offers.names());
    // Claude Code's hooks report to the node over a socket in its folder.
    let agents = offers.agents.clone();
    let hooks_dir = state.dir.clone();
    let _hooks = AbortOnDropHandle::new(tokio::spawn(async move {
        if let Err(e) = agents.external().listen_hooks(&hooks_dir).await {
            log::warn!("not taking Claude Code hooks: {e:#}");
        }
    }));
    let mut wait = FIRST_RETRY;
    loop {
        match connect(&endpoint, state.server.into(), &state.name, &offers).await {
            Ok(()) => {
                log::info!("the server closed the connection");
                wait = FIRST_RETRY;
            }
            Err(e) => log::warn!("connection lost: {e:#}"),
        }
        sleep(wait).await;
        wait = (wait * 2).min(LAST_RETRY);
    }
}

/// One connection to `server`, served until it closes: the hello, the
/// environment report, then sessions, in the node's own protocols and
/// `offers`. `Ok` when the server closed it. Shell sessions end with the
/// connection; agent sessions live in `offers.agents`.
pub async fn connect(
    endpoint: &Endpoint,
    server: EndpointAddr,
    name: &str,
    offers: &Offers,
) -> Result<()> {
    open(endpoint, server, name, offers).await?.serve().await
}

/// What a node serves beside its shell.
#[derive(Clone, Default)]
pub struct Offers {
    /// The coding agents and their sessions, made once so the sessions
    /// outlive each connection.
    pub agents: Arc<Agents>,
    /// Protocols the host adds, such as the Mac app's `desktop/1`.
    pub extra: Vec<Arc<dyn Protocol>>,
}

impl Offers {
    fn names(&self) -> Vec<String> {
        std::iter::once(shell::PROTOCOL.to_owned())
            .chain(self.agents.offers())
            .chain(self.extra.iter().map(|protocol| protocol.name().to_owned()))
            .collect()
    }
}

/// A connection the server welcomed, not yet served.
pub struct Linked {
    connection: Connection,
    tx: LinkTx,
    rx: LinkRx,
    offers: Offers,
}

/// Connects to `server` and says hello; `Ok` once the server welcomed this
/// node.
pub async fn open(
    endpoint: &Endpoint,
    server: EndpointAddr,
    name: &str,
    offers: &Offers,
) -> Result<Linked> {
    let connection = timeout(CONNECT_TIMEOUT, endpoint.connect(server, NODE_ALPN))
        .await
        .context("connect timed out")??;
    let (send, recv) = connection.open_bi().await?;
    let (mut tx, mut rx) = link(send, recv);
    tx.send(&NodeHello {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        name: name.to_owned(),
        offers: offers.names(),
    })
    .await?;
    match rx.recv::<Down>().await {
        Ok(Some(Down::Welcome {})) => {}
        Ok(Some(other)) => bail!("expected a welcome, got {other:?}"),
        Ok(None) | Err(_) => match connection.close_reason() {
            Some(reason) => bail!("the server refused this node: {reason}"),
            None => bail!("the server closed before its welcome"),
        },
    }
    log::info!("connected as {name:?}");
    Ok(Linked {
        connection,
        tx,
        rx,
        offers: offers.clone(),
    })
}

impl Linked {
    /// Reports the machine and serves sessions until the connection closes.
    /// `Ok` when the server closed it.
    pub async fn serve(self) -> Result<()> {
        let Self {
            connection,
            mut tx,
            mut rx,
            offers,
        } = self;
        serve_linked(&connection, &mut tx, &mut rx, &offers).await
    }
}

async fn serve_linked(
    connection: &Connection,
    tx: &mut LinkTx,
    rx: &mut LinkRx,
    offers: &Offers,
) -> Result<()> {
    let mut reported = env::probe().await;
    tx.send(&Up::Env(reported.clone())).await?;
    let mut reprobe = interval(REPROBE);
    reprobe.tick().await;
    // A probe runs a dozen commands; it runs beside the loop, not in it.
    let mut probing: Option<AbortOnDropHandle<EnvReport>> = None;
    // Streams end with the connection: dropping the set stops them.
    let mut sessions = JoinSet::new();
    loop {
        tokio::select! {
            stream = connection.accept_bi() => {
                let (send, recv) = stream?;
                if sessions.len() >= MAX_SESSIONS {
                    log::warn!("refused a session: {MAX_SESSIONS} already open");
                    continue;
                }
                sessions.spawn(session(send, recv, offers.clone()));
            }
            Some(ended) = sessions.join_next() => match ended {
                Ok(Ok(())) => {}
                Ok(Err(e)) => log::debug!("session ended: {e:#}"),
                Err(e) => log::warn!("session task failed: {e}"),
            },
            message = rx.recv::<Down>() => match message {
                Ok(Some(message)) => log::debug!("server sent {message:?}"),
                Ok(None) => return Ok(()),
                Err(e) if connection.close_reason().is_none() => log::debug!("skipped a message: {e:#}"),
                Err(e) => return Err(e),
            },
            _ = reprobe.tick(), if probing.is_none() => {
                probing = Some(AbortOnDropHandle::new(tokio::spawn(env::probe())));
            }
            Some(probed) = async { probing.as_mut()?.await.into() }, if probing.is_some() => {
                probing = None;
                match probed {
                    Ok(now) if changed(&reported, &now) => {
                        tx.send(&Up::Env(now.clone())).await?;
                        reported = now;
                    }
                    Ok(_) => {}
                    Err(e) => log::warn!("probing the machine failed: {e}"),
                }
            }
        }
    }
}

/// Serves one session stream in the protocol it opens with.
async fn session(send: SendStream, recv: RecvStream, offers: Offers) -> Result<()> {
    let (mut tx, mut rx) = link(send, recv);
    let open: Open = timeout(OPEN_TIMEOUT, rx.recv())
        .await
        .context("the session did not say its protocol")??
        .context("the session closed before its protocol")?;
    match open.protocol.as_str() {
        shell::PROTOCOL => {
            tx.send(&Opened::Ready {}).await?;
            shell::serve(tx, rx).await
        }
        agent::SESSIONS_PROTOCOL if !offers.agents.offers().is_empty() => {
            tx.send(&Opened::Ready {}).await?;
            crate::agents::serve_sessions(tx, rx, offers.agents).await
        }
        other => match offers.agents.kind(other).cloned() {
            Some(kind) => {
                tx.send(&Opened::Ready {}).await?;
                crate::agents::serve_session(tx, rx, offers.agents, kind).await
            }
            None => {
                if let Some(protocol) = offers.extra.iter().find(|p| p.name() == other) {
                    tx.send(&Opened::Ready {}).await?;
                    return protocol.serve(tx, rx).await;
                }
                let reason = format!("this node does not offer {other}");
                tx.send(&Opened::Refused { reason }).await
            }
        },
    }
}

fn changed(before: &EnvReport, now: &EnvReport) -> bool {
    before != now
}
