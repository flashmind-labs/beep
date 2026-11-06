//! The node's key and settings, kept in one folder.

use crate::{agents::AgentKind, agents::found};
use anyhow::{Context, Result, bail};
use beep_proto::port::NODE_PORT;
use iroh::{EndpointId, SecretKey};
use serde::{Deserialize, Serialize};
use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

const KEY_FILE: &str = "key";
const SETTINGS_FILE: &str = "node.toml";

/// What `node.toml` holds.
#[derive(Debug, Serialize, Deserialize)]
struct Settings {
    /// The server's endpoint id; a node that only belongs to a mesh has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    server: Option<String>,
    name: String,
    /// Agents to offer beyond the known ones, or in their place, such as
    /// `[[agents]] name = "claude" program = "/opt/acp/claude-agent-acp"`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    agents: Vec<AgentKind>,
    /// Folders Beep may read and write files in, such as
    /// `files = ["/srv/project"]`. None by default; a project sync the mesh
    /// sets needs none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    files: Vec<PathBuf>,
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
    /// The server this node is linked to, if it is.
    pub server: Option<EndpointId>,
    pub name: String,
    pub agents: Vec<AgentKind>,
    /// The folders shared through `files/1`.
    pub files: Vec<PathBuf>,
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
        Self::save(dir, key, Some(server), name, None)
    }

    /// The node in `dir`, made with `name` and no server only when `dir` has
    /// no settings yet, for a machine that only belongs to a mesh.
    pub fn load_or_start(dir: &Path, name: &str) -> Result<Self> {
        // Settings without a key are never replaced: `load` says so.
        if dir.join(SETTINGS_FILE).exists() {
            return Self::load(dir);
        }
        let key = ensure_key(dir)?;
        Self::save(dir, key, None, name, None)
    }

    /// Saves the settings for `server` with the key `ensure_key` made.
    /// `search` replaces the folder checkouts are looked for in; `None`
    /// keeps it.
    pub fn save(
        dir: &Path,
        key: SecretKey,
        server: Option<EndpointId>,
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
        let old = old.map(|old| (old.agents, old.files, old.port));
        let (agents, files, port) = old.unwrap_or_default();
        let settings = Settings {
            server: server.map(|server| server.to_string()),
            name: name.trim().to_owned(),
            agents,
            files,
            port,
            search,
        };
        fs::write(dir.join(SETTINGS_FILE), toml::to_string(&settings)?)?;
        Ok(Self::of(dir, key, server, settings))
    }

    pub fn load(dir: &Path) -> Result<Self> {
        let Some(key) = read_key(dir)? else {
            bail!(
                "no key in {}; run `beep-node setup <token>` first",
                dir.display()
            );
        };
        let settings = read_settings(dir)?;
        let server = parse_server(&settings)?;
        Ok(Self::of(dir, key, server, settings))
    }

    fn of(dir: &Path, key: SecretKey, server: Option<EndpointId>, settings: Settings) -> Self {
        Self {
            dir: dir.to_owned(),
            key,
            server,
            name: settings.name,
            agents: settings.agents,
            files: settings.files,
            port: settings.port.unwrap_or(NODE_PORT),
            search: Search(settings.search),
        }
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
    parse_server(&read_settings(dir)?)
}

fn parse_server(settings: &Settings) -> Result<Option<EndpointId>> {
    settings
        .server
        .as_deref()
        .map(|server| {
            server
                .parse()
                .context("the server in node.toml is not an endpoint id")
        })
        .transpose()
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
    let mut file = crate::files::fsat::private(OpenOptions::new().write(true).create_new(true))
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    file.write_all(bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A machine that joins a mesh before it ever ran has no folder: starting
    /// its state makes the folder and key a service launch needs.
    #[test]
    fn a_mesh_only_node_gets_its_folder_and_key() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("not").join("yet");
        assert!(!dir.exists());
        let state = State::load_or_start(&dir, "box").unwrap();
        assert!(dir.canonicalize().is_ok());
        assert!(state.server.is_none());
        let again = State::load_or_start(&dir, "other name").unwrap();
        assert_eq!(again.key.public(), state.key.public());
    }

    #[test]
    fn linking_again_keeps_the_key() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let server = SecretKey::generate().public().to_string();
        let first = State::create(dir, &server, "box").unwrap();
        let second = State::create(dir, &server, "build box").unwrap();
        assert_eq!(first.key.public(), second.key.public());
        let loaded = State::load(dir).unwrap();
        assert_eq!(loaded.name, "build box");
        assert_eq!(loaded.key.public(), first.key.public());
    }

    #[test]
    fn a_node_with_no_server_starts_and_loads_again_without_one() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let first = State::load_or_start(dir, "box").unwrap();
        assert_eq!(first.server, None);
        let again = State::load_or_start(dir, "other name").unwrap();
        assert_eq!(again.name, "box");
        assert_eq!(again.key.public(), first.key.public());
        assert_eq!(saved_server(dir).unwrap(), None);
    }

    #[test]
    fn settings_without_a_key_are_never_replaced() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let server = SecretKey::generate().public().to_string();
        State::create(dir, &server, "box").unwrap();
        let settings = fs::read_to_string(dir.join(SETTINGS_FILE)).unwrap();
        fs::remove_file(dir.join(KEY_FILE)).unwrap();
        assert!(State::load_or_start(dir, "other").is_err());
        assert_eq!(
            fs::read_to_string(dir.join(SETTINGS_FILE)).unwrap(),
            settings
        );
        assert!(!dir.join(KEY_FILE).exists());
    }

    #[test]
    fn linking_again_keeps_settings_it_cannot_read() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let server = SecretKey::generate().public().to_string();
        State::create(dir, &server, "box").unwrap();
        let broken = "files = [\"/srv/project\"\n";
        fs::write(dir.join(SETTINGS_FILE), broken).unwrap();
        assert!(State::create(dir, &server, "box").is_err());
        assert_eq!(fs::read_to_string(dir.join(SETTINGS_FILE)).unwrap(), broken);
    }

    /// `webhook_listen` was a setting before webhooks went (#339): a
    /// node.toml that still has it loads, and saving drops it.
    #[test]
    fn an_old_webhook_listen_setting_is_ignored() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let server = SecretKey::generate().public().to_string();
        State::create(dir, &server, "box").unwrap();
        let path = dir.join(SETTINGS_FILE);
        let old = fs::read_to_string(&path).unwrap() + "webhook_listen = \"[fd00::12]:7480\"\n";
        fs::write(&path, old).unwrap();
        assert_eq!(State::load(dir).unwrap().name, "box");
        State::create(dir, &server, "box").unwrap();
        assert!(
            !fs::read_to_string(&path)
                .unwrap()
                .contains("webhook_listen")
        );
    }
}
