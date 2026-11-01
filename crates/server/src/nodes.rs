//! Nodes: machines running `beep-node` that Beep runs work on (see
//! `beep_proto::node`). Each keeps one connection open on `NODE_ALPN`; the
//! server notes what it reports and keeps the connection so sessions can be
//! opened on it. Only admins may connect one for now, and a single-user
//! server's settings can turn them off.

use crate::{
    app::App,
    instance::Privilege,
    protocol::{
        DeviceKind,
        link::{LinkOffer, MAX_NAME},
        node::{
            AppName, CLOSE_SETUP_REFUSED, Down, EnvReport, LinkRx, LinkTx, MAX_FRAME, NodeHello,
            NodeSetup, Up, link,
        },
        setup::{SetupToken, TOKEN_TTL},
    },
    push::CLOSE_REVOKED,
    store::{Claim, ClaimNode, ClaimedNode, LinkKey, NewKey, NewNode, NodeRow},
    user::User,
};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use iroh::{
    EndpointId,
    endpoint::{Connection, VarInt},
    protocol::{AcceptError, ProtocolHandler},
};
use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak},
    time::SystemTime,
};
use tokio::{sync::Notify, time::timeout};
use tokio_util::task::AbortOnDropHandle;

mod agent;
mod deliver;
mod desktop;
mod files;
mod follow;
mod shell;
mod stats;
mod sync;
mod tasks;
pub mod transfer;

pub use crate::protocol::node::{
    NODE_ALPN, desktop::PROTOCOL as DESKTOP, files::PROTOCOL as FILES, shell::PROTOCOL as SHELL,
    sync::PROTOCOL as SYNC, transfer::PROTOCOL as TRANSFER,
};
pub use agent::{AgentLink, Mark, Watched};
pub(crate) use deliver::agent_name;
pub use deliver::{Ask, Deliveries, Post};
pub use follow::{Follower, Follows};
pub use shell::{Exited, Ran};
pub use stats::Sampled;
pub use sync::{Refusal, SyncState, push_sync};

const CLOSE_NOT_ALLOWED: u32 = 401;
/// The line the machines section starts with, also how a chat finds the
/// one it last got.
pub(crate) const MACHINES_HEADER: &str = "The user's machines (exec, process, desktop and \
    code_agent run on the one `in` names; send_file copies files between them):";
const CLOSE_UNAVAILABLE: u32 = 503;

/// A node with its connection open.
#[derive(Clone)]
pub struct Online {
    pub user: String,
    pub name: String,
    pub offers: Vec<String>,
    /// The folder the user's files and code live in, when this node is
    /// their environment (`beep-node --workspace`).
    pub workspace: Option<String>,
    pub connection: Connection,
    /// Held weakly: the app owns the nodes.
    app: Weak<App>,
    shell: Arc<shell::ShellSlot>,
    agents: Arc<agent::AgentLinks>,
    desktop: Arc<desktop::DesktopSlot>,
    stats: Arc<stats::StatsSlot>,
    /// Woken when the user's credentials change, for a node that takes
    /// them.
    credentials: Option<Arc<Notify>>,
}

/// The nodes connected now, by key.
#[derive(Default)]
pub struct Nodes {
    online: Mutex<HashMap<EndpointId, Online>>,
    /// The server's own key, which link offers are signed for. Set when the
    /// node handler is made.
    server: OnceLock<EndpointId>,
    /// Sessions started outside Beep that chats follow.
    pub follows: Follows,
    /// What agent sessions tell their chats.
    pub deliveries: Deliveries,
    /// What project sync keeps between pushes.
    pub sync: SyncState,
}

impl Nodes {
    fn lock(&self) -> MutexGuard<'_, HashMap<EndpointId, Online>> {
        self.online.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The server's key, once it listens for nodes.
    pub fn server(&self) -> Option<EndpointId> {
        self.server.get().copied()
    }

    /// The user's environment: a node of theirs online now that keeps a
    /// workspace and offers the shell and files it needs.
    pub fn workspace(&self, user: &str) -> Option<Online> {
        self.lock()
            .values()
            .find(|online| {
                online.user == user
                    && online.workspace.is_some()
                    && online.offers(SHELL)
                    && online.offers(FILES)
            })
            .cloned()
    }

    /// The user's nodes online now.
    pub fn of_user(&self, user: &str) -> Vec<Online> {
        self.lock()
            .values()
            .filter(|online| online.user == user)
            .cloned()
            .collect()
    }

    /// The connected node with the key `node`, as the store keeps it.
    pub fn online(&self, node: &str) -> Option<Online> {
        let id: EndpointId = node.parse().ok()?;
        self.lock().get(&id).cloned()
    }

    /// Tells the user's nodes that take credentials to fetch them again.
    pub fn credentials_changed(&self, user: &str) {
        for online in self.lock().values().filter(|online| online.user == user) {
            if let Some(changed) = &online.credentials {
                changed.notify_one();
            }
        }
    }

    fn insert(&self, node: EndpointId, online: Online) {
        // A node that reconnects replaces its old connection.
        if let Some(old) = self.lock().insert(node, online) {
            old.connection.close(VarInt::from_u32(0), b"replaced");
        }
    }

    /// Drops `node`'s connection, as when its key is removed (`reason` is
    /// `CLOSE_REVOKED`) or its account merged into another and it must sign
    /// in again (`0`).
    pub fn forget(&self, node: &EndpointId, reason: u32) {
        if let Some(online) = self.lock().remove(node) {
            online
                .connection
                .close(VarInt::from_u32(reason), b"signed out");
        }
    }

    /// Forgets `node` if `connection` is still the one it holds.
    fn remove(&self, node: &EndpointId, connection: &Connection) {
        let mut nodes = self.lock();
        if nodes
            .get(node)
            .is_some_and(|online| online.connection.stable_id() == connection.stable_id())
        {
            nodes.remove(node);
        }
    }
}

#[derive(Clone)]
pub struct NodeHandler {
    app: Arc<App>,
}

impl fmt::Debug for NodeHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeHandler").finish()
    }
}

impl NodeHandler {
    /// `server` is the key of the endpoint the handler serves on.
    pub fn new(app: Arc<App>, server: EndpointId) -> Self {
        if app.nodes.server.set(server).is_err() && app.nodes.server() != Some(server) {
            log::warn!("node handler made again for another server key");
        }
        Self { app }
    }
}

impl ProtocolHandler for NodeHandler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let endpoint = connection.remote_id();
        // The hello comes first: it may carry a setup token that links this
        // key before it signs in.
        // A key nobody linked gets only a small hello read, so a stranger
        // cannot make the server buffer megabytes before it is refused.
        let linked = match self.app.store.linked_sign_in(&endpoint).await {
            Ok(sign_in) => sign_in.is_some(),
            Err(e) => {
                log::debug!("node {endpoint} looked up as unlinked: {e:#}");
                false
            }
        };
        let (tx, rx, mut hello) = match read_hello(&connection, linked).await {
            Ok(opened) => opened,
            Err(e) => {
                log::debug!("node {endpoint} sent no hello: {e:#}");
                connection.close(VarInt::from_u32(CLOSE_UNAVAILABLE), b"no hello");
                return Ok(());
            }
        };
        let mut claimed = None;
        if let Some(setup) = hello.setup.take() {
            match claim(&self.app, &endpoint, &setup, &hello.name).await {
                Ok(node) => {
                    hello.name = node.name.clone();
                    claimed = Some(node);
                }
                Err(Refused::Setup(reason)) => {
                    log::info!("node setup refused; key={endpoint} reason={reason:?}");
                    connection.close(VarInt::from_u32(CLOSE_SETUP_REFUSED), reason.as_bytes());
                    return Ok(());
                }
                Err(Refused::Failed(e)) => {
                    log::warn!("node setup failed: {e:#}");
                    connection.close(VarInt::from_u32(CLOSE_UNAVAILABLE), b"unavailable");
                    return Ok(());
                }
            }
        }
        // Otherwise a node joins an account with a link code or setup token
        // first; an unknown key gets no account of its own here.
        let user = match self.app.sign_in_linked(&endpoint).await {
            Ok(Some(signed_in)) => signed_in.user,
            Ok(None) => {
                connection.close(VarInt::from_u32(CLOSE_REVOKED), b"not linked");
                return Ok(());
            }
            Err(e) => {
                log::warn!("node sign in failed: {e:#}");
                connection.close(VarInt::from_u32(CLOSE_UNAVAILABLE), b"unavailable");
                return Ok(());
            }
        };
        if !self.app.allows(Privilege::Nodes, &user) {
            connection.close(VarInt::from_u32(CLOSE_NOT_ALLOWED), b"nodes are for admins");
            return Ok(());
        }
        let control = Control {
            tx,
            rx,
            hello,
            claimed,
        };
        if let Err(e) = serve(&self.app, user, &connection, control).await {
            log::debug!("node {endpoint} left: {e:#}");
        }
        self.app.nodes.remove(&endpoint, &connection);
        Ok(())
    }
}

/// The control stream once its hello is read.
struct Control {
    tx: LinkTx,
    rx: LinkRx,
    hello: NodeHello,
    /// The token the hello claimed, when it carried one.
    claimed: Option<ClaimedNode>,
}

/// The longest hello read from a key nobody linked: a name, a version, a
/// few offers and a setup token fit in far less.
const MAX_UNLINKED_HELLO: usize = 16 * 1024;

async fn read_hello(connection: &Connection, linked: bool) -> Result<(LinkTx, LinkRx, NodeHello)> {
    let (send, recv) = timeout(crate::transport::READ_TIMEOUT, connection.accept_bi())
        .await
        .context("no control stream")??;
    let (tx, mut rx) = link(send, recv);
    if !linked {
        rx.set_max_frame(MAX_UNLINKED_HELLO);
    }
    let hello: NodeHello = timeout(crate::transport::READ_TIMEOUT, rx.recv())
        .await
        .context("no hello")??
        .context("closed before its hello")?;
    rx.set_max_frame(MAX_FRAME);
    Ok((tx, rx, hello))
}

/// Why a setup token did not link the node.
enum Refused {
    /// In words for the user, sent as the close reason.
    Setup(String),
    Failed(anyhow::Error),
}

impl From<anyhow::Error> for Refused {
    fn from(e: anyhow::Error) -> Self {
        Self::Failed(e)
    }
}

/// Claims the hello's setup token for the node's key `endpoint`, whose
/// hello calls it `fallback`.
async fn claim(
    app: &Arc<App>,
    endpoint: &EndpointId,
    setup: &NodeSetup,
    fallback: &str,
) -> Result<ClaimedNode, Refused> {
    let refuse = |reason: &str| Err(Refused::Setup(reason.to_owned()));
    let Ok(token) = SetupToken::parse(&setup.token) else {
        return refuse("That is not a setup token. Copy the whole command from Beep again.");
    };
    if app.nodes.server() != Some(token.server) {
        return refuse("This setup token is for another server.");
    }
    let named = setup.name.as_deref().map(str::trim);
    if named.is_some_and(|name| name.is_empty() || name.len() > MAX_NAME) {
        return refuse("The machine's name is empty or too long.");
    }
    let fallback = match fallback.trim() {
        name if name.is_empty() || name.len() > MAX_NAME => "Machine",
        name => name,
    };
    let hash = token.hash();
    let claim = ClaimNode {
        hash: &hash,
        endpoint: *endpoint,
        named,
        fallback,
    };
    match app.store.claim_node_token(claim).await? {
        Claim::Linked(node) => {
            log::info!(
                "node linked by setup token; user={} key={endpoint}",
                node.account.id
            );
            Ok(node)
        }
        Claim::Unknown => refuse("This setup token is unknown here. Make a new one in Beep."),
        Claim::Expired => refuse("This setup token expired. Make a new one in Beep."),
        Claim::Used => refuse("This setup token was already used. Make a new one in Beep."),
        Claim::OtherAccount => refuse(
            "This machine's key belongs to another Beep account. On the machine, delete \
             beep-node's key folder (beep-node in its config folder, or the --dir you gave) \
             and run setup again.",
        ),
    }
}

/// Reads the control stream until the node goes away.
async fn serve(
    app: &Arc<App>,
    user: Arc<User>,
    connection: &Connection,
    control: Control,
) -> Result<()> {
    let endpoint = connection.remote_id();
    let Control {
        mut tx,
        mut rx,
        hello,
        claimed,
    } = control;
    user.node_connected(NewNode {
        endpoint,
        name: &hello.name,
        version: &hello.version,
    })
    .await?;
    if let Some(claimed) = claimed {
        // Tells the user's devices their list of devices changed, as a
        // link offer does.
        user.changed("");
        tx.send(&Down::Linked {
            account: claimed.account_name,
            name: claimed.name,
        })
        .await?;
    }
    tx.send(&Down::Welcome {}).await?;
    log::info!(
        "node connected; user={} name={:?} version={} offers={:?} workspace={:?}",
        user.id(),
        hello.name,
        hello.version,
        hello.offers,
        hello.workspace
    );
    // Only a node that said it takes credentials gets them; the first
    // set goes at once.
    let credentials = hello.credentials.then(|| Arc::new(Notify::new()));
    if let Some(changed) = &credentials {
        changed.notify_one();
    }
    let stats = Arc::new(stats::StatsSlot::default());
    app.nodes.insert(
        endpoint,
        Online {
            user: user.id().to_owned(),
            name: hello.name,
            offers: hello.offers,
            workspace: hello.workspace,
            connection: connection.clone(),
            app: Arc::downgrade(app),
            shell: Arc::default(),
            agents: Arc::default(),
            desktop: Arc::default(),
            stats: stats.clone(),
            credentials: credentials.clone(),
        },
    );
    app.nodes.follows.reconnected(app, &endpoint.to_string());
    // Sends the node its synced projects, and its peers how it is reached
    // now. Ends with the connection.
    let _push = {
        let (app, user) = (app.clone(), user.clone());
        AbortOnDropHandle::new(tokio::spawn(async move {
            if let Err(e) = push_sync(&app.nodes, &user, Some(endpoint)).await {
                log::warn!("sync list not sent; node={endpoint} error={e:#}");
            }
        }))
    };
    loop {
        tokio::select! {
            message = rx.recv::<Up>() => match message {
                Ok(Some(Up::Env(env))) => {
                    // A report that fails to save is not worth dropping the node.
                    if let Err(e) = user.save_node_env(&endpoint, &env).await {
                        log::warn!("node report not saved; node={endpoint} error={e:#}");
                    }
                }
                Ok(Some(Up::Stats(report))) => stats.set(report),
                Ok(Some(other)) => log::debug!("node sent something unhandled: {other:?}"),
                Ok(None) => return Ok(()),
                // A message from a newer node this server cannot read yet.
                Err(e) if connection.close_reason().is_none() => {
                    log::debug!("node message skipped: {e:#}")
                }
                Err(e) => bail!(e),
            },
            () = changed(credentials.as_deref()) => {
                match user.machine_credentials(&app.credential_key).await {
                    Ok(set) => tx.send(&Down::Credentials(set)).await?,
                    // The node keeps what it had; the next change tries again.
                    Err(e) => log::warn!("credentials not sent; node={endpoint} error={e:#}"),
                }
            }
        }
    }
}

/// Finishes when `notify` is woken; never without one.
async fn changed(notify: Option<&Notify>) {
    match notify {
        Some(notify) => notify.notified().await,
        None => std::future::pending().await,
    }
}

/// Links the machine whose `beep-node link` printed `text` (its
/// `beep-link:` link) to `user`, checked as `LinkDevice` checks an offer the
/// phone scanned. Gives the machine's name.
pub async fn link_machine(nodes: &Nodes, user: &User, text: &str) -> Result<String> {
    let text = text.trim();
    let offer = match LinkOffer::parse(text) {
        Ok(offer) => offer,
        Err(_) if is_check_code(text) => bail!(
            "That is the six-character check code. Pass the whole `beep-link:` link that \
             `beep-node link` printed below it."
        ),
        Err(e) => bail!("Not a `beep-link:` link: {e}."),
    };
    let server = nodes
        .server()
        .context("This server does not take machines.")?;
    offer
        .verify(server, SystemTime::now())
        .map_err(|e| anyhow::anyhow!("Could not link it: {e}."))?;
    if offer.device == server {
        bail!("That is this server's own link code, not a machine's.");
    }
    let name = offer.name.trim();
    let key = NewKey {
        endpoint: offer.device,
        name,
        kind: DeviceKind::Desktop {},
    };
    match user.link_key(key).await? {
        LinkKey::Linked(_) => {
            // Tells the user's devices their list of devices changed.
            user.changed("");
            Ok(name.to_owned())
        }
        LinkKey::Taken => bail!("That machine is already linked to an account."),
    }
}

/// A setup token `create_token` made.
pub struct NewToken {
    pub token: SetupToken,
    pub expires_at: DateTime<Utc>,
}

/// Why `create_token` made none.
#[derive(Debug, thiserror::Error)]
pub enum TokenRefusal {
    #[error("Only admins can add machines on this server.")]
    NotAllowed,
    #[error("This server does not take machines.")]
    NoServer,
    #[error("The machine's name is empty or too long.")]
    BadName,
    #[error(transparent)]
    Failed(#[from] anyhow::Error),
}

/// A one-time token that links the machine running `beep-node setup
/// <token>` to `user`, for `TOKEN_TTL`. `name` is what to call it.
pub async fn create_token(
    app: &App,
    user: &User,
    name: Option<&str>,
) -> Result<NewToken, TokenRefusal> {
    if !app.allows(Privilege::Nodes, user) {
        return Err(TokenRefusal::NotAllowed);
    }
    let server = app.nodes.server().ok_or(TokenRefusal::NoServer)?;
    let name = name.map(str::trim).filter(|name| !name.is_empty());
    if name.is_some_and(|name| name.len() > MAX_NAME) {
        return Err(TokenRefusal::BadName);
    }
    let token = SetupToken::generate(server);
    let expires_at = user
        .create_node_token(&token.hash(), name, TOKEN_TTL)
        .await?;
    Ok(NewToken { token, expires_at })
}

/// Six letters and digits, as the check code `beep-node link` prints.
fn is_check_code(text: &str) -> bool {
    let code: String = text.split_whitespace().collect();
    code.len() == 6 && code.chars().all(|c| c.is_ascii_alphanumeric())
}

/// The machines section of a turn's context: each machine on a line with
/// whether it is online, its OS and which coding agents are signed in.
pub(crate) fn machines(nodes: &[NodeRow], online: &Nodes) -> Option<String> {
    if nodes.is_empty() {
        return None;
    }
    let mut out = String::from(MACHINES_HEADER);
    for node in nodes {
        let mut facts = vec![if online.online(&node.endpoint).is_some() {
            "online".to_owned()
        } else {
            "offline".to_owned()
        }];
        if let Some(env) = &node.env {
            facts.push(env.os_version.clone());
            for login in &env.logins {
                facts.push(match (login.signed_in, &login.account) {
                    (true, Some(account)) => format!("{} signed in as {account}", login.tool),
                    (true, None) => format!("{} signed in", login.tool),
                    (false, _) => format!("{} not signed in", login.tool),
                });
            }
        }
        facts.retain(|fact| !fact.is_empty());
        out.push_str(&format!("\n- {}: {}", node.name, facts.join("; ")));
    }
    Some(out)
}

/// A node's report as a few lines for the model.
pub fn describe(env: &EnvReport) -> String {
    let mut lines = vec![format!(
        "{} {} ({}), {} CPUs, {} GB memory, host {}, home {}",
        env.os,
        env.os_version,
        env.arch,
        env.cpus,
        env.memory_mb / 1024,
        env.hostname,
        env.home
    )];
    if !env.tools.is_empty() {
        let tools: Vec<_> = env
            .tools
            .iter()
            .map(|tool| match &tool.version {
                Some(version) => format!("{} ({version})", tool.name),
                None => tool.name.clone(),
            })
            .collect();
        lines.push(format!("Tools: {}", tools.join(", ")));
    }
    for gpu in &env.gpus {
        lines.push(match gpu.vram_mb {
            Some(mb) => format!("GPU: {} ({} GB)", gpu.model, (mb + 512) / 1024),
            None => format!("GPU: {}", gpu.model),
        });
    }
    let known_apps: Vec<_> = env
        .apps
        .iter()
        .filter(|app| app.name != AppName::Unknown)
        .collect();
    if !known_apps.is_empty() {
        let apps: Vec<_> = known_apps
            .into_iter()
            .map(|app| match &app.version {
                Some(version) => format!("{} {version}", app.name),
                None => app.name.to_string(),
            })
            .collect();
        lines.push(format!("Apps: {}", apps.join(", ")));
    }
    let capabilities: Vec<&'static str> = env.capabilities().into_iter().map(Into::into).collect();
    if !capabilities.is_empty() {
        lines.push(format!("Can: {}", capabilities.join(", ")));
    }
    for login in &env.logins {
        lines.push(match (login.signed_in, &login.account) {
            (true, Some(account)) => format!("{}: signed in as {account}", login.tool),
            (true, None) => format!("{}: signed in", login.tool),
            (false, _) => format!("{}: not signed in", login.tool),
        });
    }
    if !env.avds.is_empty() {
        lines.push(format!("Android emulators: {}", env.avds.join(", ")));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::node::{AppInfo, GpuInfo, GpuVendor, Login, ToolInfo};

    #[test]
    fn describe_names_capabilities() {
        let env = EnvReport {
            gpus: vec![GpuInfo {
                vendor: GpuVendor::Nvidia,
                model: "NVIDIA GeForce RTX 4090".into(),
                vram_mb: Some(24564),
                driver: None,
            }],
            apps: vec![
                AppInfo {
                    name: AppName::Blender,
                    version: Some("4.2".into()),
                },
                AppInfo {
                    name: AppName::UnrealEngine,
                    version: Some("5.4".into()),
                },
            ],
            ..EnvReport::default()
        };
        let text = describe(&env);
        assert!(
            text.contains("GPU: NVIDIA GeForce RTX 4090 (24 GB)"),
            "{text}"
        );
        assert!(
            text.contains("Apps: Blender 4.2, Unreal Engine 5.4"),
            "{text}"
        );
        assert!(text.contains("Can: gpu, cuda, blender, unreal"), "{text}");
    }

    #[test]
    fn describe_skips_unknown_apps() {
        let env = EnvReport {
            apps: vec![AppInfo {
                name: AppName::Unknown,
                version: None,
            }],
            ..EnvReport::default()
        };
        assert!(!describe(&env).contains("Apps:"));
    }

    #[test]
    fn the_machines_section_shows_each_machine_on_a_line() {
        let key = iroh::SecretKey::generate().public();
        let other = iroh::SecretKey::generate().public();
        let node = |endpoint: &EndpointId, name: &str, env: Option<EnvReport>| NodeRow {
            endpoint: endpoint.to_string(),
            name: name.into(),
            version: "0.1.0".into(),
            env,
            seen_at: chrono::Utc::now(),
        };
        let mac = EnvReport {
            os_version: "macOS 26.3".into(),
            arch: "aarch64".into(),
            hostname: "studio".into(),
            cpus: 12,
            home: "/Users/someone".into(),
            logins: vec![Login {
                tool: "gh".into(),
                signed_in: false,
                account: None,
            }],
            avds: vec!["Pixel_7".into()],
            ..EnvReport::default()
        };
        let nodes = [
            node(&key, "mac", Some(mac)),
            node(&other, "build box", None),
        ];
        assert_eq!(
            machines(&nodes, &Nodes::default()).unwrap(),
            format!(
                "{MACHINES_HEADER}\n\
                 - mac: offline; macOS 26.3; gh not signed in\n\
                 - build box: offline"
            )
        );
        assert_eq!(machines(&[], &Nodes::default()), None);
    }

    #[test]
    fn a_report_reads_as_lines() {
        let env = EnvReport {
            os: "linux".into(),
            os_version: "Ubuntu 24.04".into(),
            arch: "x86_64".into(),
            hostname: "build-box".into(),
            cpus: 16,
            memory_mb: 65536,
            home: "/home/user".into(),
            tools: vec![
                ToolInfo {
                    name: "git".into(),
                    version: Some("2.51.0".into()),
                },
                ToolInfo {
                    name: "docker".into(),
                    version: None,
                },
            ],
            logins: vec![Login {
                tool: "gh".into(),
                signed_in: false,
                account: None,
            }],
            avds: vec![],
            ..EnvReport::default()
        };
        assert_eq!(
            describe(&env),
            "linux Ubuntu 24.04 (x86_64), 16 CPUs, 64 GB memory, host build-box, home /home/user\n\
             Tools: git (2.51.0), docker\n\
             Can: docker\n\
             gh: not signed in"
        );
    }
}
