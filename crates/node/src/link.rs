//! The connection to the server: the link code that joins the account, then
//! the control stream, kept open and reopened when it drops.

use crate::{
    agents::Agents, env, files::Folders, setup::Setup, shell, state::State, tasks,
    transfer::Transfers,
};
use anyhow::{Context, Result, bail};
use beep_proto::{
    link::LinkOffer,
    node::{
        CLOSE_SETUP_REFUSED, Down, EnvReport, LinkRx, LinkTx, NODE_ALPN, NodeHello, NodeSetup,
        Open, Opened, Up, agent, link,
    },
};
use iroh::{
    Endpoint, EndpointAddr,
    endpoint::{Connection, ConnectionError, RecvStream, SendStream, VarInt, presets},
};
use std::{
    future::Future,
    path::Path,
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

/// Stays connected, waiting longer after each failed try, until `stop`
/// finishes; then closes the agent sessions and returns. With `workspace`,
/// this node is the user's environment: Beep keeps their files and runs its
/// code there, and it is shared through `files/1` before the folders in
/// `node.toml`.
pub async fn serve(
    state: State,
    workspace: Option<&Path>,
    stop: impl Future<Output = ()>,
) -> Result<()> {
    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(state.key.clone())
        .bind()
        .await?;
    log::info!("node key {}", endpoint.id());
    // Made once, so agent sessions outlive each connection.
    // Installs gh, teacli and the agent skills beside serving; the work
    // stops with the node.
    let setup = Setup::new(&state.dir);
    let _setup = setup.start();
    let mut offers = Offers {
        agents: Arc::new(Agents::new(crate::agents::detect(&state.agents))),
        extra: Vec::new(),
        setup: Some(setup),
    };
    // Files only when the owner shared folders in node.toml, or this node
    // is the user's environment.
    let shared: Vec<_> = workspace
        .map(Path::to_path_buf)
        .into_iter()
        .chain(state.files.iter().cloned())
        .collect();
    let folders = Folders::new(&shared);
    // The workspace as the node resolved it, only once it is shared.
    let workspace = workspace
        .and_then(|dir| dir.canonicalize().ok())
        .filter(|dir| folders.first() == Some(dir.as_path()))
        .map(|dir| dir.to_string_lossy().into_owned());
    let mut started = None;
    if !folders.is_empty() {
        // Transfers reach the same folders; without its store the node
        // still serves the rest.
        match Transfers::start(&endpoint, &state.dir.join("blobs"), folders.clone()).await {
            Ok(transfers) => {
                offers.extra.push(Arc::new(transfers.syncs()));
                offers.extra.push(Arc::new(transfers.clone()));
                started = Some(transfers);
            }
            Err(e) => log::warn!("not offering transfers: {e:#}"),
        }
        offers.extra.push(Arc::new(folders));
    }
    let hello = Hello {
        name: &state.name,
        workspace: workspace.as_deref(),
        setup: None,
    };
    log::info!("offering {:?}", offers.names());
    // Claude Code's hooks report to the node over a socket in its folder.
    let agents = offers.agents.clone();
    let hooks_dir = state.dir.clone();
    let _hooks = AbortOnDropHandle::new(tokio::spawn(async move {
        if let Err(e) = agents.external().listen_hooks(&hooks_dir).await {
            log::warn!("not taking Claude Code hooks: {e:#}");
        }
    }));
    let stay = async {
        let mut wait = FIRST_RETRY;
        loop {
            match connect_as(&endpoint, state.server.into(), &hello, &offers).await {
                Ok(()) => {
                    log::info!("the server closed the connection");
                    wait = FIRST_RETRY;
                }
                Err(e) => log::warn!("connection lost: {e:#}"),
            }
            sleep(wait).await;
            wait = (wait * 2).min(LAST_RETRY);
        }
    };
    tokio::select! {
        () = stay => {}
        () = stop => {}
    }
    log::info!("stopping the agent sessions");
    offers.agents.shutdown().await;
    if let Some(transfers) = started
        && let Err(e) = transfers.shutdown().await
    {
        log::warn!("transfers did not stop cleanly: {e:#}");
    }
    Ok(())
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
    let hello = Hello {
        name,
        workspace: None,
        setup: None,
    };
    connect_as(endpoint, server, &hello, offers).await
}

/// What the hello says about this node beside its offers.
struct Hello<'a> {
    name: &'a str,
    workspace: Option<&'a str>,
    /// A setup token to claim (`beep-node setup`).
    setup: Option<&'a NodeSetup>,
}

/// What claiming a setup token gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claimed {
    /// The account's name, such as mega-fox-trot.
    pub account: String,
    /// What the server calls this machine.
    pub name: String,
}

/// Connects to `server` once to claim `setup` with this endpoint's key,
/// then closes. `name` is the machine's name when neither `setup` nor the
/// token gives one. Errors say why in words for the user.
pub async fn claim(
    endpoint: &Endpoint,
    server: EndpointAddr,
    name: &str,
    setup: &NodeSetup,
) -> Result<Claimed> {
    let hello = Hello {
        name,
        workspace: None,
        setup: Some(setup),
    };
    let linked = open_as(endpoint, server, &hello, &Offers::default()).await?;
    linked
        .connection
        .close(VarInt::from_u32(0), b"set up; reconnecting");
    linked
        .claimed
        .context("the server welcomed this node without linking it")
}

async fn connect_as(
    endpoint: &Endpoint,
    server: EndpointAddr,
    hello: &Hello<'_>,
    offers: &Offers,
) -> Result<()> {
    open_as(endpoint, server, hello, offers)
        .await?
        .serve()
        .await
}

/// What a node serves beside its shell.
#[derive(Clone, Default)]
pub struct Offers {
    /// The coding agents and their sessions, made once so the sessions
    /// outlive each connection.
    pub agents: Arc<Agents>,
    /// Protocols the host adds, such as the Mac app's `desktop/1`.
    pub extra: Vec<Arc<dyn Protocol>>,
    /// Takes the user's credentials and installs the tools they are for.
    /// Without it the node does not ask for credentials.
    pub setup: Option<Arc<Setup>>,
}

impl Offers {
    fn names(&self) -> Vec<String> {
        [shell::PROTOCOL.to_owned(), tasks::PROTOCOL.to_owned()]
            .into_iter()
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
    /// What the hello's setup token gave, when it carried one.
    claimed: Option<Claimed>,
}

/// Connects to `server` and says hello; `Ok` once the server welcomed this
/// node.
pub async fn open(
    endpoint: &Endpoint,
    server: EndpointAddr,
    name: &str,
    offers: &Offers,
) -> Result<Linked> {
    let hello = Hello {
        name,
        workspace: None,
        setup: None,
    };
    open_as(endpoint, server, &hello, offers).await
}

async fn open_as(
    endpoint: &Endpoint,
    server: EndpointAddr,
    hello: &Hello<'_>,
    offers: &Offers,
) -> Result<Linked> {
    let name = hello.name;
    let connection = timeout(CONNECT_TIMEOUT, endpoint.connect(server, NODE_ALPN))
        .await
        .context("connect timed out")??;
    let (send, recv) = connection.open_bi().await?;
    let (mut tx, mut rx) = link(send, recv);
    tx.send(&NodeHello {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        name: name.to_owned(),
        offers: offers.names(),
        workspace: hello.workspace.map(str::to_owned),
        setup: hello.setup.cloned(),
        credentials: offers.setup.is_some(),
    })
    .await?;
    let mut claimed = None;
    loop {
        match rx.recv::<Down>().await {
            Ok(Some(Down::Welcome {})) => break,
            Ok(Some(Down::Linked { account, name })) if hello.setup.is_some() => {
                claimed = Some(Claimed { account, name });
            }
            Ok(Some(other)) => bail!("expected a welcome, got {other:?}"),
            Ok(None) | Err(_) => match connection.close_reason() {
                Some(reason) => bail!("{}", refusal(&reason, hello.setup.is_some())),
                None => bail!("the server closed before its welcome"),
            },
        }
    }
    log::info!("connected as {name:?}");
    Ok(Linked {
        connection,
        tx,
        rx,
        offers: offers.clone(),
        claimed,
    })
}

/// Why the server closed before its welcome, in words for the user.
fn refusal(reason: &ConnectionError, setting_up: bool) -> String {
    if let ConnectionError::ApplicationClosed(close) = reason {
        let text = String::from_utf8_lossy(&close.reason);
        if close.error_code == VarInt::from_u32(CLOSE_SETUP_REFUSED) {
            return text.into_owned();
        }
        // A server from before setup tokens turns the unknown key away
        // without reading the hello.
        if setting_up && text == "not linked" {
            return "This server does not take setup tokens yet. Update it, or link the \
                    machine with `beep-node link`."
                .to_owned();
        }
    }
    format!("the server refused this node: {reason}")
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
            claimed: _,
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
                Ok(Some(Down::Credentials(credentials))) => match &offers.setup {
                    Some(setup) => setup.apply(credentials).await,
                    None => log::debug!("credentials came to a node without a setup"),
                },
                Ok(Some(message)) => log::debug!("server sent {message:?}"),
                Ok(None) => return Ok(()),
                Err(e) if connection.close_reason().is_none() => log::debug!("skipped a message: {e:#}"),
                Err(e) => return Err(e),
            },
            _ = reprobe.tick(), if probing.is_none() => {
                probing = Some(AbortOnDropHandle::new(tokio::spawn(env::probe())));
            }
            // A tool was installed: report it without waiting for the tick.
            () = tool_changed(offers.setup.as_deref()), if probing.is_none() => {
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
        tasks::PROTOCOL => {
            tx.send(&Opened::Ready {}).await?;
            tasks::serve(tx, rx).await
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

/// Finishes when `setup` installed a tool; never without one.
async fn tool_changed(setup: Option<&Setup>) {
    match setup {
        Some(setup) => setup.changed().await,
        None => std::future::pending().await,
    }
}

fn changed(before: &EnvReport, now: &EnvReport) -> bool {
    before != now
}
