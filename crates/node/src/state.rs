//! The node's key and settings, kept in one folder.

use crate::{agents::AgentKind, found};
use anyhow::{Context, Result, bail};
use beep_proto::port::NODE_PORT;
use iroh::{EndpointId, SecretKey};
use serde::{Deserialize, Serialize};
use std::{
    fmt, fs,
    net::SocketAddr,
    path::{Path, PathBuf},
};

const KEY_FILE: &str = "key";
const SETTINGS_FILE: &str = "node.toml";

/// What `node.toml` holds.
#[derive(Debug, Serialize, Deserialize)]
struct Settings {
    /// The server's endpoint id.
    server: String,
    name: String,
    /// Agents to offer beyond the known ones, or in their place, such as
    /// `[[agents]] name = "claude" program = "/opt/acp/claude-agent-acp"`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    agents: Vec<AgentKind>,
    /// Folders Beep may read and write files in, such as
    /// `files = ["/srv/project"]`. None by default.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    files: Vec<PathBuf>,
    /// Where Forgejo and Gitea servers on the private network deliver
    /// webhooks for live project updates: this machine's address on that
    /// network and a port, such as `webhook_listen = "[fd00::12]:7480"`.
    /// Unset, only GitHub repositories get live updates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    webhook_listen: Option<SocketAddr>,
    /// The UDP port the node's iroh endpoint binds, for a firewall rule;
    /// `NODE_PORT` unset, random at 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    port: Option<u16>,
    /// Where to look for git checkouts, such as `search = "~/Projects"`;
    /// home when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    search: Option<PathBuf>,
}

pub struct State {
    /// The folder the key and settings live in.
    pub dir: PathBuf,
    pub key: SecretKey,
    pub server: EndpointId,
    pub name: String,
    pub agents: Vec<AgentKind>,
    /// The folders shared through `files/1`.
    pub files: Vec<PathBuf>,
    /// Where webhooks for live project updates come to from Forgejo and
    /// Gitea servers (`webhook_listen` in `node.toml`).
    pub webhook_listen: Option<SocketAddr>,
    /// The UDP port to bind (`port` in `node.toml`).
    pub port: u16,
    /// Where to look for git checkouts (`search` in `node.toml`).
    pub search: Search,
}

/// Where the node looks for git checkouts: a folder as written, where `~`
/// is home; home when unset.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Search(Option<PathBuf>);

impl Search {
    /// A folder as the user typed it; empty, `~` and `~/` are home.
    pub fn parse(text: &str) -> Self {
        let text = text.trim();
        let home = matches!(text, "" | "~" | "~/" | "~\\");
        Self((!home).then(|| PathBuf::from(text)))
    }

    /// The folder to walk, `~` made home; `None` when it needs a home and
    /// this system has none.
    pub fn root(&self) -> Option<PathBuf> {
        expand(self.0.as_deref(), dirs::home_dir())
    }

    /// The folder to walk, refused unless it is a folder on this machine.
    pub fn existing_root(&self) -> Result<PathBuf> {
        match self.root() {
            Some(root) if root.is_dir() => Ok(root),
            _ => bail!("{self} is not a folder on this machine"),
        }
    }
}

/// `~/Projects` is `Projects` under home.
impl fmt::Display for Search {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Some(path) => write!(f, "{}", path.display()),
            None => f.write_str("~/"),
        }
    }
}

fn expand(search: Option<&Path>, home: Option<PathBuf>) -> Option<PathBuf> {
    let Some(search) = search else {
        return home;
    };
    match search.strip_prefix("~") {
        Ok(rest) if rest.as_os_str().is_empty() => home,
        Ok(rest) => Some(home?.join(rest)),
        Err(_) if search.is_absolute() => Some(search.to_owned()),
        Err(_) => Some(home?.join(search)),
    }
}

pub fn default_dir() -> Result<PathBuf> {
    Ok(dirs::config_dir()
        .context("this system has no config folder; pass --dir")?
        .join("beep-node"))
}

impl State {
    /// Saves the settings and makes the key, keeping an existing one so the
    /// machine stays the same device.
    pub fn create(dir: &Path, server: &str, name: &str) -> Result<Self> {
        let server: EndpointId = server.parse().context("--server is not an endpoint id")?;
        let key = ensure_key(dir)?;
        Self::save(dir, key, server, name, None)
    }

    /// Saves the settings for `server` with the key `ensure_key` made.
    /// `search` replaces the folder checkouts are looked for in; `None`
    /// keeps it.
    pub fn save(
        dir: &Path,
        key: SecretKey,
        server: EndpointId,
        name: &str,
        search: Option<Search>,
    ) -> Result<Self> {
        // Linking again keeps the agents and folders set by hand, and
        // refuses rather than drop settings it cannot read.
        let old = if dir.join(SETTINGS_FILE).exists() {
            Some(read_settings(dir)?)
        } else {
            None
        };
        let kept = old.as_ref().and_then(|old| old.search.clone());
        let search = match search {
            Some(search) => {
                if search.0 != kept {
                    found::forget_checkouts(dir)?;
                }
                search.0
            }
            None => kept,
        };
        let old = old.map(|old| (old.agents, old.files, old.webhook_listen, old.port));
        let (agents, files, webhook_listen, port) = old.unwrap_or_default();
        let settings = Settings {
            server: server.to_string(),
            name: name.trim().to_owned(),
            agents,
            files,
            webhook_listen,
            port,
            search,
        };
        fs::write(dir.join(SETTINGS_FILE), toml::to_string(&settings)?)?;
        Ok(Self {
            dir: dir.to_owned(),
            key,
            server,
            name: settings.name,
            agents: settings.agents,
            files: settings.files,
            webhook_listen: settings.webhook_listen,
            port: settings.port.unwrap_or(NODE_PORT),
            search: Search(settings.search),
        })
    }

    pub fn load(dir: &Path) -> Result<Self> {
        let Some(key) = read_key(dir)? else {
            bail!(
                "no key in {}; run `beep-node setup <token>` first",
                dir.display()
            );
        };
        let settings = read_settings(dir)?;
        Ok(Self {
            dir: dir.to_owned(),
            key,
            server: settings
                .server
                .parse()
                .context("the server in node.toml is not an endpoint id")?,
            name: settings.name,
            agents: settings.agents,
            files: settings.files,
            webhook_listen: settings.webhook_listen,
            port: settings.port.unwrap_or(NODE_PORT),
            search: Search(settings.search),
        })
    }
}

/// This machine's key, made and saved in `dir` when it has none, so the
/// machine stays the same device.
pub fn ensure_key(dir: &Path) -> Result<SecretKey> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    match read_key(dir)? {
        Some(key) => Ok(key),
        None => {
            let key = SecretKey::generate();
            write_private(&dir.join(KEY_FILE), &key.to_bytes())?;
            Ok(key)
        }
    }
}

/// The server `node.toml` in `dir` names, `None` before any link.
pub fn saved_server(dir: &Path) -> Result<Option<EndpointId>> {
    if !dir.join(SETTINGS_FILE).exists() {
        return Ok(None);
    }
    let server = read_settings(dir)?
        .server
        .parse()
        .context("the server in node.toml is not an endpoint id")?;
    Ok(Some(server))
}

/// Where `node.toml` in `dir` says to look for git checkouts.
pub fn saved_search(dir: &Path) -> Result<Search> {
    Ok(Search(read_settings(dir)?.search))
}

/// Has `node.toml` in `dir` look for git checkouts in `search`, and drops
/// the checkouts found elsewhere.
pub fn save_search(dir: &Path, search: &Search) -> Result<()> {
    let mut settings = read_settings(dir)?;
    settings.search = search.0.clone();
    fs::write(dir.join(SETTINGS_FILE), toml::to_string(&settings)?)?;
    found::forget_checkouts(dir)
}

fn read_settings(dir: &Path) -> Result<Settings> {
    let path = dir.join(SETTINGS_FILE);
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("reading {}", path.display()))
}

fn read_key(dir: &Path) -> Result<Option<SecretKey>> {
    let path = dir.join(KEY_FILE);
    match fs::read(&path) {
        Ok(bytes) => {
            let bytes: [u8; 32] = bytes
                .try_into()
                .map_err(|_| anyhow::anyhow!("{} is not a key", path.display()))?;
            Ok(Some(SecretKey::from_bytes(&bytes)))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Writes a file only its owner can read.
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::{fs::OpenOptions, io::Write};
    let mut file = crate::fsat::private(OpenOptions::new().write(true).create_new(true))
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    file.write_all(bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("beep-node-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn linking_again_keeps_the_key() {
        let dir = temp_dir("relink");
        let server = SecretKey::generate().public().to_string();
        let first = State::create(&dir, &server, "box").unwrap();
        let second = State::create(&dir, &server, "build box").unwrap();
        assert_eq!(first.key.public(), second.key.public());
        let loaded = State::load(&dir).unwrap();
        assert_eq!(loaded.name, "build box");
        assert_eq!(loaded.key.public(), first.key.public());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn linking_again_keeps_settings_it_cannot_read() {
        let dir = temp_dir("relink-broken");
        let server = SecretKey::generate().public().to_string();
        State::create(&dir, &server, "box").unwrap();
        let broken = "files = [\"/srv/project\"\n";
        fs::write(dir.join(SETTINGS_FILE), broken).unwrap();
        assert!(State::create(&dir, &server, "box").is_err());
        assert_eq!(fs::read_to_string(dir.join(SETTINGS_FILE)).unwrap(), broken);
        fs::remove_dir_all(&dir).unwrap();
    }
}
