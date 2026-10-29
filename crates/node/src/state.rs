//! The node's key and settings, kept in one folder.

use crate::agents::AgentKind;
use anyhow::{Context, Result, bail};
use iroh::{EndpointId, SecretKey};
use serde::{Deserialize, Serialize};
use std::{
    fs,
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
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let key = match read_key(dir)? {
            Some(key) => key,
            None => {
                let key = SecretKey::generate();
                write_private(&dir.join(KEY_FILE), &key.to_bytes())?;
                key
            }
        };
        // Linking again keeps the agents and folders set by hand.
        let (agents, files) = read_settings(dir)
            .map(|old| (old.agents, old.files))
            .unwrap_or_default();
        let settings = Settings {
            server: server.to_string(),
            name: name.trim().to_owned(),
            agents,
            files,
        };
        fs::write(dir.join(SETTINGS_FILE), toml::to_string(&settings)?)?;
        Ok(Self {
            dir: dir.to_owned(),
            key,
            server,
            name: settings.name,
            agents: settings.agents,
            files: settings.files,
        })
    }

    pub fn load(dir: &Path) -> Result<Self> {
        let Some(key) = read_key(dir)? else {
            bail!("no key in {}; run `beep-node link` first", dir.display());
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
        })
    }
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
    use std::{fs::OpenOptions, io::Write, os::unix::fs::OpenOptionsExt};
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
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
}
