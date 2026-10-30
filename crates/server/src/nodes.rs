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
        link::LinkOffer,
        node::{Down, EnvReport, NodeHello, Up, link},
    },
    push::CLOSE_REVOKED,
    store::{LinkKey, MemoryRow, NewKey, NewNode, NodeRow, Scope},
    user::User,
};
use anyhow::{Context, Result, bail};
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
use tokio::time::timeout;

mod agent;
mod deliver;
mod desktop;
mod files;
mod follow;
mod shell;

pub use crate::protocol::node::{
    NODE_ALPN, desktop::PROTOCOL as DESKTOP, shell::PROTOCOL as SHELL,
};
pub use agent::{AgentLink, Mark, Watched};
pub use deliver::{Ask, Deliveries, Post};
pub use follow::{Follower, Follows};
pub use shell::Ran;
pub(crate) use shell::head_and_tail;

const CLOSE_NOT_ALLOWED: u32 = 401;
/// The line the machines section starts with, also how a chat finds the
/// one it last got.
pub(crate) const MACHINES_HEADER: &str = "The user's machines (the node tool runs work on them):";
/// The most memories shown under each machine, newest first.
const MACHINE_MEMORIES: usize = 5;
const CLOSE_UNAVAILABLE: u32 = 503;

/// A node with its connection open.
#[derive(Clone)]
pub struct Online {
    pub user: String,
    pub name: String,
    pub offers: Vec<String>,
    pub connection: Connection,
    /// Held weakly: the app owns the nodes.
    app: Weak<App>,
    shell: Arc<shell::ShellSlot>,
    agents: Arc<agent::AgentLinks>,
    desktop: Arc<desktop::DesktopSlot>,
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
}

impl Nodes {
    fn lock(&self) -> MutexGuard<'_, HashMap<EndpointId, Online>> {
        self.online.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The server's key, once it listens for nodes.
    pub fn server(&self) -> Option<EndpointId> {
        self.server.get().copied()
    }

    /// The connected node with the key `node`, as the store keeps it.
    pub fn online(&self, node: &str) -> Option<Online> {
        let id: EndpointId = node.parse().ok()?;
        self.lock().get(&id).cloned()
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
        // A node joins an account with a link code first; an unknown key gets
        // no account of its own here.
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
        if let Err(e) = serve(&self.app, &user, &connection).await {
            log::debug!("node {endpoint} left: {e:#}");
        }
        self.app.nodes.remove(&endpoint, &connection);
        Ok(())
    }
}

/// Reads the control stream until the node goes away.
async fn serve(app: &Arc<App>, user: &User, connection: &Connection) -> Result<()> {
    let endpoint = connection.remote_id();
    let (send, recv) = connection.accept_bi().await?;
    let (mut tx, mut rx) = link(send, recv);
    let hello: NodeHello = timeout(crate::transport::READ_TIMEOUT, rx.recv())
        .await
        .context("no hello")??
        .context("closed before its hello")?;
    user.node_connected(NewNode {
        endpoint,
        name: &hello.name,
        version: &hello.version,
    })
    .await?;
    tx.send(&Down::Welcome {}).await?;
    log::info!(
        "node connected; user={} name={:?} version={} offers={:?}",
        user.id(),
        hello.name,
        hello.version,
        hello.offers
    );
    app.nodes.insert(
        endpoint,
        Online {
            user: user.id().to_owned(),
            name: hello.name,
            offers: hello.offers,
            connection: connection.clone(),
            app: Arc::downgrade(app),
            shell: Arc::default(),
            agents: Arc::default(),
            desktop: Arc::default(),
        },
    );
    app.nodes.follows.reconnected(app, &endpoint.to_string());
    loop {
        match rx.recv::<Up>().await {
            Ok(Some(Up::Env(env))) => {
                // A report that fails to save is not worth dropping the node.
                if let Err(e) = user.save_node_env(&endpoint, &env).await {
                    log::warn!("node report not saved; node={endpoint} error={e:#}");
                }
            }
            Ok(Some(other)) => log::debug!("node sent something unhandled: {other:?}"),
            Ok(None) => return Ok(()),
            // A message from a newer node this server cannot read yet.
            Err(e) if connection.close_reason().is_none() => {
                log::debug!("node message skipped: {e:#}")
            }
            Err(e) => bail!(e),
        }
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

/// Six letters and digits, as the check code `beep-node link` prints.
fn is_check_code(text: &str) -> bool {
    let code: String = text.split_whitespace().collect();
    code.len() == 6 && code.chars().all(|c| c.is_ascii_alphanumeric())
}

/// The machines section of a turn's context: each machine on a line with
/// what decides where work goes, then its newest memories. `rows` are the
/// user's memories, newest first.
pub(crate) fn machines(nodes: &[NodeRow], online: &Nodes, rows: &[MemoryRow]) -> Option<String> {
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
            facts.push(
                [env.os_version.as_str(), env.arch.as_str()]
                    .iter()
                    .filter(|part| !part.is_empty())
                    .copied()
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            for login in &env.logins {
                facts.push(match (login.signed_in, &login.account) {
                    (true, Some(account)) => format!("{} signed in as {account}", login.tool),
                    (true, None) => format!("{} signed in", login.tool),
                    (false, _) => format!("{} not signed in", login.tool),
                });
            }
            if !env.avds.is_empty() {
                facts.push(format!("Android emulators {}", env.avds.join(", ")));
            }
        }
        facts.retain(|fact| !fact.is_empty());
        out.push_str(&format!("\n- {}: {}", node.name, facts.join("; ")));
        let key = node.endpoint.parse().ok().map(Scope::Machine);
        for row in rows
            .iter()
            .filter(|row| key.as_ref() == Some(&row.scope))
            .take(MACHINE_MEMORIES)
        {
            out.push_str(&format!("\n  - {}", row.content));
        }
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
    use crate::protocol::node::{Login, ToolInfo};

    #[test]
    fn the_machines_section_shows_each_machine_with_its_memories() {
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
            logins: vec![Login {
                tool: "gh".into(),
                signed_in: false,
                account: None,
            }],
            avds: vec!["Pixel_7".into()],
            ..EnvReport::default()
        };
        let memory = |id: &str, content: &str, scope: Scope| MemoryRow {
            id: id.into(),
            content: content.into(),
            category: "unfiled".into(),
            source: crate::protocol::MemorySource::Learned {},
            updated_at: String::new(),
            valid: Default::default(),
            origin: None,
            core: false,
            scope,
        };
        let rows = [
            memory("1", "Emulators run here.", Scope::Machine(key)),
            memory("2", "Lives in Lisbon.", Scope::User),
            memory(
                "3",
                "Has the beep repo in /srv/beep.",
                Scope::Machine(other),
            ),
        ];
        let nodes = [
            node(&key, "mac", Some(mac)),
            node(&other, "build box", None),
        ];
        assert_eq!(
            machines(&nodes, &Nodes::default(), &rows).unwrap(),
            format!(
                "{MACHINES_HEADER}\n\
                 - mac: offline; macOS 26.3 aarch64; gh not signed in; Android emulators Pixel_7\n  \
                 - Emulators run here.\n\
                 - build box: offline\n  \
                 - Has the beep repo in /srv/beep."
            )
        );
        assert_eq!(machines(&[], &Nodes::default(), &rows), None);
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
        };
        assert_eq!(
            describe(&env),
            "linux Ubuntu 24.04 (x86_64), 16 CPUs, 64 GB memory, host build-box, home /home/user\n\
             Tools: git (2.51.0), docker\n\
             gh: not signed in"
        );
    }
}
