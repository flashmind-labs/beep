//! Single-user instance mode: a server that one person runs for themselves.
//!
//! An instance has one owner account. The owner's key can be named in the
//! config; otherwise the first key that connects claims it. Every other key
//! is refused unless the owner linked it with `LinkDevice`. The owner counts
//! as an admin, and the features only admins get on a shared server are
//! settings here, on by default.
//!
//! The bundled `beep-node` joins the owner without a link code: the server
//! writes its key to a folder both share (`server.id`), the node writes its
//! own back (`node.id`), and when that key connects the server links it to
//! the owner.

use anyhow::{Context, Result};
use beep_proto::link::LinkOffer;
use iroh::{EndpointId, SecretKey};
use qrcode::{QrCode, render::unicode::Dense1x2};
use serde::Deserialize;
use std::{
    fs::{self, Permissions},
    io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{PoisonError, RwLock},
    time::SystemTime,
};

/// Where the server writes its key for the bundled node.
pub const SERVER_FILE: &str = "server.id";
/// Where the bundled node writes its key and name, one per line.
pub const NODE_FILE: &str = "node.id";
/// What the bundled node is called when its file names nothing.
const NODE_NAME: &str = "Beep server";

/// The config's `[instance]` table. Its presence turns instance mode on.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceConfig {
    /// The owner's key. Without it the first key to connect owns the server.
    pub owner: Option<EndpointId>,
    /// What the server calls itself in its link code.
    #[serde(default = "default_name")]
    pub name: String,
    /// Account events wake the agent (see `crate::events`).
    #[serde(default = "on")]
    pub events: bool,
    /// Once turned background sub-agents off. Unused now; kept so configs
    /// that set it still load.
    #[serde(default = "on")]
    pub background: bool,
    /// Machines running `beep-node`.
    #[serde(default = "on")]
    pub nodes: bool,
    /// Once turned the removed per-prompt agents off. Unused now; kept so
    /// configs that set it still load.
    #[serde(default = "on")]
    pub agents: bool,
    /// The folder shared with the bundled node.
    pub local_node: Option<PathBuf>,
}

fn on() -> bool {
    true
}

fn default_name() -> String {
    "Beep".to_owned()
}

/// What only admins get on a shared server, and what an instance's
/// settings turn off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Privilege {
    Events,
    Nodes,
}

/// The bundled node, as its file names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalNode {
    pub endpoint: EndpointId,
    pub name: String,
}

/// A running instance: its settings and its owner once known.
pub struct Instance {
    config: InstanceConfig,
    /// The owner's account id, once claimed or read back.
    owner: RwLock<Option<String>>,
}

impl Instance {
    pub fn new(config: InstanceConfig) -> Self {
        Self {
            config,
            owner: RwLock::default(),
        }
    }

    /// Whether the settings leave `privilege` on.
    pub fn allows(&self, privilege: Privilege) -> bool {
        match privilege {
            Privilege::Events => self.config.events,
            Privilege::Nodes => self.config.nodes,
        }
    }

    /// The owner's key from the config, the only one that may claim.
    pub fn owner_key(&self) -> Option<&EndpointId> {
        self.config.owner.as_ref()
    }

    pub fn name(&self) -> &str {
        &self.config.name
    }

    /// The owner's account, when known.
    pub(crate) fn owner(&self) -> Option<String> {
        self.owner
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn set_owner(&self, account: &str) {
        *self.owner.write().unwrap_or_else(PoisonError::into_inner) = Some(account.to_owned());
    }

    /// Writes the server's key where the bundled node reads it.
    pub fn announce(&self, server: EndpointId) -> Result<()> {
        let Some(dir) = &self.config.local_node else {
            return Ok(());
        };
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        write_replacing(&dir.join(SERVER_FILE), &format!("{server}\n"))
    }

    /// The bundled node, once it wrote its file.
    pub(crate) fn local_node(&self) -> Result<Option<LocalNode>> {
        let Some(dir) = &self.config.local_node else {
            return Ok(None);
        };
        let path = dir.join(NODE_FILE);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        parse_node_file(&text).with_context(|| format!("reading {}", path.display()))
    }
}

/// The server's offer of itself, as a QR code and as text, for the apps
/// to scan or paste.
pub fn link_code(key: &SecretKey, name: &str, now: SystemTime) -> Result<String> {
    let offer = LinkOffer::for_server(key, name, now)?;
    let uri = offer.to_uri();
    let qr = QrCode::new(uri.as_bytes())
        .context("making the QR code")?
        .render::<Dense1x2>()
        .quiet_zone(true)
        .build();
    Ok(format!(
        "Scan this with Beep on your phone to connect it to this server.\n\
         The first phone to connect owns the server.\n\n\
         {qr}\n\n  Code: {code}\n  Link: {uri}\n  Server: {server}\n\n\
         The code works for 10 minutes; `beep-server link` shows a new one.\n",
        code = offer.code(),
        server = key.public(),
    ))
}

fn parse_node_file(text: &str) -> Result<Option<LocalNode>> {
    let mut lines = text.lines().map(str::trim);
    let Some(id) = lines.next().filter(|id| !id.is_empty()) else {
        return Ok(None);
    };
    let endpoint = id.parse().context("not a node key")?;
    let name = lines
        .next()
        .filter(|name| !name.is_empty() && name.len() <= beep_proto::link::MAX_NAME)
        .unwrap_or(NODE_NAME)
        .to_owned();
    Ok(Some(LocalNode { endpoint, name }))
}

/// Writes `path` whole, so a reader never sees half of it. Readable by
/// all whatever the umask: the node runs as another user, and the file
/// only holds a public key.
fn write_replacing(path: &Path, text: &str) -> Result<()> {
    let partial = path.with_extension("partial");
    fs::write(&partial, text).with_context(|| format!("writing {}", partial.display()))?;
    fs::set_permissions(&partial, Permissions::from_mode(0o644))
        .with_context(|| format!("opening up {}", partial.display()))?;
    fs::rename(&partial, path).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(text: &str) -> InstanceConfig {
        toml::from_str(text).expect("a valid instance table")
    }

    #[test]
    fn an_agents_key_still_loads() {
        let instance = Instance::new(config("agents = false"));
        assert!(instance.allows(Privilege::Events));
        assert!(instance.allows(Privilege::Nodes));
    }

    #[test]
    fn the_link_code_names_this_server() {
        let key = SecretKey::generate();
        let now = SystemTime::now();
        let text = link_code(&key, "Beep", now).expect("a link code");
        let uri = text
            .lines()
            .find_map(|line| line.trim().strip_prefix("Link: "))
            .expect("the link line");
        let offer = LinkOffer::parse(uri).expect("an offer");
        assert_eq!(offer.server(now), Some(key.public()));
        assert!(text.contains(&offer.code()));
    }

    #[test]
    fn every_privilege_is_on_by_default() {
        let instance = Instance::new(config(""));
        for privilege in [Privilege::Events, Privilege::Nodes] {
            assert!(instance.allows(privilege), "{privilege:?} is on");
        }
        assert_eq!(instance.name(), "Beep");
        assert!(instance.owner_key().is_none());
        let off = Instance::new(config("events = false\nnodes = false"));
        assert!(!off.allows(Privilege::Events));
        assert!(!off.allows(Privilege::Nodes));
    }

    #[test]
    fn the_server_and_node_files_go_both_ways() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let instance = Instance::new(InstanceConfig {
            local_node: Some(dir.path().join("link")),
            ..config("")
        });
        assert_eq!(instance.local_node().expect("no file is fine"), None);
        let server = SecretKey::generate().public();
        instance
            .announce(server)
            .expect("the server file is written");
        let written =
            fs::read_to_string(dir.path().join("link").join(SERVER_FILE)).expect("the server file");
        assert_eq!(written.trim(), server.to_string());

        let node = SecretKey::generate().public();
        fs::write(
            dir.path().join("link").join(NODE_FILE),
            format!("{node}\nbuild box\n"),
        )
        .expect("the node file is written");
        assert_eq!(
            instance.local_node().expect("a node file"),
            Some(LocalNode {
                endpoint: node,
                name: "build box".to_owned(),
            })
        );
        fs::write(dir.path().join("link").join(NODE_FILE), format!("{node}\n"))
            .expect("the node file is written");
        assert_eq!(
            instance.local_node().expect("a node file").map(|n| n.name),
            Some(NODE_NAME.to_owned())
        );
    }
}
