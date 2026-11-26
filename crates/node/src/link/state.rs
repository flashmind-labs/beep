//! The node's key and settings, kept in one folder.

pub use crate::fs::note::default_dir;
use crate::{agents::AgentKind, agents::found};
use anyhow::{Context, Result, bail};
use beep_proto::port::NODE_PORT;
use iroh::SecretKey;
use serde::{Deserialize, Serialize};
use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

const KEY_FILE: &str = "key";
const SETTINGS_FILE: &str = "node.toml";

/// What `node.toml` holds. A `server` left by an older node is ignored.
#[derive(Debug, Serialize, Deserialize)]
struct Settings {
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

impl From<&str> for Search {
    /// A folder as the user typed it; empty, `~` and `~/` are home.
    fn from(text: &str) -> Self {
        let text = text.trim();
        let home = matches!(text, "" | "~" | "~/" | "~\\");
        Self((!home).then(|| PathBuf::from(text)))
    }
}

impl Search {
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

impl State {
    /// The node in `dir`, made with `name` only when `dir` has no settings
    /// yet.
    pub fn load_or_start(dir: &Path, name: &str) -> Result<Self> {
        // Settings without a key are never replaced: `load` says so.
        if dir.join(SETTINGS_FILE).exists() {
            return Self::load(dir);
        }

        let key = ensure_key(dir)?;
        Self::save(dir, key, name)
    }

    /// Saves new settings named `name` with the key `ensure_key` made.
    fn save(dir: &Path, key: SecretKey, name: &str) -> Result<Self> {
        let settings = Settings {
            name: name.trim().to_owned(),
            agents: Vec::new(),
            files: Vec::new(),
            port: None,
            search: None,
        };
        fs::write(dir.join(SETTINGS_FILE), toml::to_string(&settings)?)?;
        Ok(Self::of(dir, key, settings))
    }

    pub fn load(dir: &Path) -> Result<Self> {
        let Some(key) = read_key(dir)? else {
            bail!(
                "no key in {}; run `beep-node run` or `beep-node mesh join <ticket>` first",
                dir.display()
            );
        };
        Ok(Self::of(dir, key, read_settings(dir)?))
    }

    fn of(dir: &Path, key: SecretKey, settings: Settings) -> Self {
        Self {
            dir: dir.to_owned(),
            key,
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
fn ensure_key(dir: &Path) -> Result<SecretKey> {
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

/// Where `node.toml` in `dir` says to look for git checkouts; home when
/// `dir` has none, as the desktop app's own node, which searches home.
pub fn saved_search(dir: &Path) -> Result<Search> {
    if !dir.join(SETTINGS_FILE).exists() {
        return Ok(Search::default());
    }

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
    let mut file = crate::fs::fsat::private(OpenOptions::new().write(true).create_new(true))
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    file.write_all(bytes)?;
    Ok(())
}

#[cfg(test)]
#[path = "../tests/link_state.rs"]
mod tests;
