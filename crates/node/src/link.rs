//! The connection to the server: the link code that joins the account, then
//! the control stream, kept open and reopened when it drops.

use crate::{
    agents::{AgentKind, Agents},
    env,
    files::Folders,
    mcp::{self, Upstream},
    provision::Provision,
    restart::{self, Reopen},
    shell,
    state::State,
    stats, tasks, terminal,
    transfer::Transfers,
    webhook::Webhooks,
};
use anyhow::{Context, Result, bail};
use beep_proto::{
    client::CLOSE_REVOKED,
    link::LinkOffer,
    node::{
        CLOSE_SETUP_REFUSED, Down, EnvReport, LinkRx, LinkTx, NODE_ALPN, NodeHello, NodeSetup,
        Open, Opened, StatsReport, Up, agent, link, tool_update,
    },
};
use iroh::{
    Endpoint, EndpointAddr,
    endpoint::{Connection, ConnectionError, RecvStream, SendStream, VarInt, presets},
};
use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::{
    task::JoinSet,
    time::{MissedTickBehavior, interval, sleep, timeout},
};
use tokio_util::task::AbortOnDropHandle;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const FIRST_RETRY: Duration = Duration::from_secs(1);
const LAST_RETRY: Duration = Duration::from_secs(60);
/// How often the machine is probed again for changes.
const REPROBE: Duration = Duration::from_secs(10 * 60);
/// How often free disk and memory are sampled.
const RESAMPLE: Duration = Duration::from_secs(60);
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
    let provision = Provision::new(&state.dir);
    let _provision = provision.start();
    let mut offers = Offers::local(&state.agents, provision);
    // Only the CLI updates itself; the Mac app's node updates with the app.
    #[cfg(feature = "cli")]
    offers
        .extra
        .push(Arc::new(crate::update::Updates::new(offers.agents.clone())));
    // Live project updates: GitHub through `gh webhook forward`, Forgejo
    // and Gitea on the private-network address the settings name.
    match Webhooks::new(state.webhook_listen) {
        Ok(webhooks) => offers.extra.push(Arc::new(webhooks)),
        Err(e) => log::warn!("not taking webhooks: {e:#}"),
    }
    // Files only when the owner shared folders in node.toml, or this node
    // is the user's environment.
    let shared: Vec<_> = workspace
        .map(Path::to_path_buf)
        .into_iter()
        .chain(state.files.iter().cloned())
        .collect();
    let folders = Folders::new(&shared);
    offers.agents.share_folders(shared.clone());
    // The agents' logs read once ahead, so the first file request for a
    // session started outside Beep does not wait for them.
    let agents = offers.agents.clone();
    let _warm = AbortOnDropHandle::new(tokio::spawn(async move { agents.warm().await }));
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
                offers.extra.push(Arc::new(transfers.syncs().controls()));
                offers.extra.push(Arc::new(transfers.syncs().rescans()));
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
    // The agents' Beep MCP server runs this binary against a socket in the
    // node's folder.
    let _mcp = mcp::start(&state.dir, &offers);
    // `beep-node update` asks the node to hold still over another one.
    let agents = offers.agents.clone();
    let control_dir = state.dir.clone();
    let _control = AbortOnDropHandle::new(tokio::spawn(async move {
        if let Err(e) = restart::listen(&control_dir, agents).await {
            log::warn!("updates cannot wait for idle sessions: {e:#}");
        }
    }));
    // What was open before a restart or crash opens again: terminals now,
    // sessions once the server sent the agent settings they start with.
    // Only then is the note kept to what is open, so a start that dies
    // before leaves it as it was.
    let reopen = Reopen::read(&state.dir);
    terminal::reopen(reopen.terminals);
    let agents = offers.agents.clone();
    let note_dir = state.dir.clone();
    let mut keeping = AbortOnDropHandle::new(tokio::spawn(async move {
        agents.reopen(reopen.sessions).await;
        restart::keep(&note_dir, &agents).await;
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
    // Agents that end from here on ended with the node, whatever their exit.
    offers.agents.begin_stopping();
    // Stopped, the machine keeps no token: teacli's config, its shim and
    // the env vars go.
    if let Some(provision) = &offers.provision {
        provision.forget().await;
    }
    // Noted while they still run: the service manager ends them once the
    // node exits.
    let terminals = terminal::running();
    log::info!("stopping the agent sessions");
    offers.agents.shutdown().await;
    // The last word is the shutdown's: the keeper stops first, and its
    // write in flight lands before this one.
    keeping.abort();
    if let Err(e) = (&mut keeping).await
        && !e.is_cancelled()
    {
        log::warn!("the note of what is open was not kept: {e}");
    }
    let reopen = Reopen {
        sessions: offers.agents.open_sessions(),
        terminals,
    };
    if let Err(e) = reopen.save(&state.dir) {
        log::warn!("what was open will not open again: {e:#}");
    }
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
    pub provision: Option<Arc<Provision>>,
    /// The server's connection for the Beep MCP server's calls, set while
    /// one that takes them is connected.
    pub tools: Upstream,
}

impl Offers {
    /// What every node serves, the CLI and the Mac app alike: the coding
    /// agents found on the PATH (with `agents` from `node.toml`) and their
    /// sessions, and `provision`. A host adds its own protocols to `extra`.
    pub fn local(agents: &[AgentKind], provision: Arc<Provision>) -> Self {
        Self {
            agents: Arc::new(Agents::new(crate::agents::detect(agents))),
            extra: Vec::new(),
            provision: Some(provision),
            tools: Upstream::default(),
        }
    }

    fn names(&self) -> Vec<String> {
        [
            shell::PROTOCOL.to_owned(),
            tasks::PROTOCOL.to_owned(),
            terminal::PROTOCOL.to_owned(),
        ]
        .into_iter()
        .chain(self.agents.offers())
        .chain(
            self.provision
                .is_some()
                .then(|| tool_update::PROTOCOL.to_owned()),
        )
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
    /// The server takes `tools/1` calls.
    tools: bool,
    /// The workspace the hello named, measured first for free disk.
    workspace: Option<PathBuf>,
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
        credentials: offers.provision.is_some(),
        checkouts: true,
        agent_settings: true,
        branch_checkouts: true,
    })
    .await?;
    let mut claimed = None;
    let tools = loop {
        match rx.recv::<Down>().await {
            Ok(Some(Down::Welcome { tools })) => break tools,
            Ok(Some(Down::Linked { account, name })) if hello.setup.is_some() => {
                claimed = Some(Claimed { account, name });
            }
            Ok(Some(other)) => bail!("expected a welcome, got {other:?}"),
            Ok(None) | Err(_) => match connection.close_reason() {
                Some(reason) => bail!("{}", refusal(&reason, hello.setup.is_some())),
                None => bail!("the server closed before its welcome"),
            },
        }
    };
    log::info!("connected as {name:?}");
    Ok(Linked {
        connection,
        tx,
        rx,
        offers: offers.clone(),
        claimed,
        tools,
        workspace: hello.workspace.map(PathBuf::from),
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
        // Never linked, or removed from the account by one of its apps; the
        // server closes both the same way. Only a new setup token links it.
        if !setting_up && close.error_code == VarInt::from_u32(CLOSE_REVOKED) {
            return "This machine is not linked to a Beep account, or was removed from \
                    one. To use it, link it with `beep-node setup <token>`."
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
            tools,
            workspace,
        } = self;
        let measured: Vec<PathBuf> = workspace.into_iter().chain(dirs::home_dir()).collect();
        if tools {
            offers.tools.set(Some(connection.clone()));
        }
        let served = serve_linked(&connection, &mut tx, &mut rx, &offers, &measured).await;
        offers.tools.set(None);
        // Revoked or only cut off, the node keeps no token it cannot hear
        // about being removed; the server sends them again on reconnecting.
        if let Some(provision) = &offers.provision {
            provision.forget().await;
        }
        // The harnesses' env vars may be secrets too; the server sends the
        // settings again on reconnecting.
        offers.agents.set_settings(Default::default());
        served
    }
}

async fn serve_linked(
    connection: &Connection,
    tx: &mut LinkTx,
    rx: &mut LinkRx,
    offers: &Offers,
    measured: &[PathBuf],
) -> Result<()> {
    let mut reprobe = interval(REPROBE);
    reprobe.tick().await;
    // A probe runs a dozen commands, which can take half a minute; it runs
    // beside the loop, the first one too, so the streams the server opens
    // on connecting are taken at once. Left waiting, they time out on the
    // server, and the sync list among them is not sent again until the
    // node reconnects.
    let mut reported: Option<EnvReport> = None;
    let mut probing = Some(AbortOnDropHandle::new(tokio::spawn(env::probe())));
    // The first tick is at once, so stats follow the report.
    let mut resample = interval(RESAMPLE);
    resample.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut sampling: Option<AbortOnDropHandle<StatsReport>> = None;
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
                Ok(Some(Down::Credentials(credentials))) => match &offers.provision {
                    Some(provision) => provision.apply(credentials).await,
                    None => log::debug!("credentials came to a node that does not take them"),
                },
                Ok(Some(Down::AgentSettings(settings))) => {
                    offers.agents.set_settings(settings);
                    offers.agents.settings_arrived();
                }
                Ok(Some(message)) => log::debug!("server sent {message:?}"),
                Ok(None) => return Ok(()),
                Err(e) if connection.close_reason().is_none() => log::debug!("skipped a message: {e:#}"),
                Err(e) => return Err(e),
            },
            _ = reprobe.tick(), if probing.is_none() => {
                probing = Some(AbortOnDropHandle::new(tokio::spawn(env::probe())));
            }
            // A tool was installed: report it without waiting for the tick.
            () = tool_changed(offers.provision.as_deref()), if probing.is_none() => {
                probing = Some(AbortOnDropHandle::new(tokio::spawn(env::probe())));
            }
            _ = resample.tick(), if sampling.is_none() => {
                sampling = Some(AbortOnDropHandle::new(tokio::spawn(stats::sample(measured.to_vec()))));
            }
            Some(sampled) = async { sampling.as_mut()?.await.into() }, if sampling.is_some() => {
                sampling = None;
                match sampled {
                    Ok(report) => tx.send(&Up::Stats(report)).await?,
                    Err(e) => log::warn!("sampling the machine failed: {e}"),
                }
            }
            Some(probed) = async { probing.as_mut()?.await.into() }, if probing.is_some() => {
                probing = None;
                match probed {
                    Ok(now) if reported.as_ref().is_none_or(|before| changed(before, &now)) => {
                        tx.send(&Up::Env(now.clone())).await?;
                        reported = Some(now);
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
        terminal::PROTOCOL => {
            tx.send(&Opened::Ready {}).await?;
            terminal::serve(tx, rx).await
        }
        tool_update::PROTOCOL if let Some(provision) = &offers.provision => {
            tx.send(&Opened::Ready {}).await?;
            provision.serve_updates(tx, rx).await
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

/// Finishes when `provision` installed a tool; never without one.
async fn tool_changed(provision: Option<&Provision>) {
    match provision {
        Some(provision) => provision.changed().await,
        None => std::future::pending().await,
    }
}

fn changed(before: &EnvReport, now: &EnvReport) -> bool {
    before != now
}
