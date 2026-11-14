//! A WSL distribution's name, and what `wsl.exe` and the probe inside a
//! distribution print.

use crate::machine::service::from_utf16;
use beep_proto::node::harness::HARNESSES;
use serde::{Deserialize, Serialize};
use std::{ffi::OsStr, fmt, str::FromStr};

/// A WSL distribution's name, such as `Ubuntu-24.04`: letters, digits, `.`,
/// `_` and `-`, the names `wsl.exe` registers.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct DistroName(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum NameError {
    #[error("a WSL distribution needs a name")]
    Empty,
    #[error("{0:?} is not the name of a WSL distribution")]
    Invalid(String),
}

impl FromStr for DistroName {
    type Err = NameError;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-');

        if name.is_empty() {
            return Err(NameError::Empty);
        }

        if !name.chars().all(allowed) || name.starts_with('-') {
            return Err(NameError::Invalid(name.to_owned()));
        }

        Ok(Self(name.to_owned()))
    }
}

impl fmt::Display for DistroName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for DistroName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<OsStr> for DistroName {
    fn as_ref(&self) -> &OsStr {
        self.0.as_ref()
    }
}

/// What `wsl.exe` printed: UTF-16LE unless `WSL_UTF8` is set, when it is
/// UTF-8.
pub(crate) fn decode(bytes: &[u8]) -> String {
    let utf16 = bytes.starts_with(&[0xFF, 0xFE]) || bytes.get(1) == Some(&0);

    if utf16 {
        from_utf16(bytes)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// The distributions in what `wsl.exe --list --quiet` printed, one name a
/// line, leaving out the ones apps such as Docker Desktop keep for
/// themselves.
pub(crate) fn distros(listed: &str) -> Vec<DistroName> {
    listed
        .lines()
        .map(|line| line.trim_matches(|c: char| c.is_whitespace() || c == '\0'))
        .filter_map(|line| line.parse::<DistroName>().ok())
        .filter(|name| !for_an_app(name))
        .collect()
}

/// A distribution an app runs its own engine in, such as
/// `docker-desktop-data`, which has no user to run agents for.
fn for_an_app(name: &DistroName) -> bool {
    ["docker-desktop", "rancher-desktop"]
        .iter()
        .any(|app| name.0.starts_with(app))
}

/// What the probe found inside a distribution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Found {
    /// The coding agent CLIs, in the order Beep lists harnesses.
    pub(crate) agents: Vec<String>,
    /// The version of the beep-node installed in it.
    pub(crate) node: Option<String>,
    /// That node's id in the mesh.
    pub(crate) machine_id: Option<String>,
}

/// What `probe::SCRIPT` printed: `agent <name> <path>` for each agent on
/// the `PATH`, `node beep-node <version>` and `id <id>` when beep-node is
/// installed. Agents under `/mnt/` are Windows installs seen through the
/// `PATH` WSL appends, and do not count.
pub(crate) fn found(printed: &str) -> Found {
    let mut found = Found::default();

    for line in printed.lines() {
        let mut words = line.split_whitespace();

        match (words.next(), words.next(), words.next()) {
            (Some("agent"), Some(agent), Some(path)) if !path.starts_with("/mnt/") => {
                if !found.agents.iter().any(|known| known == agent) {
                    found.agents.push(agent.to_owned());
                }
            }
            (Some("node"), Some("beep-node"), Some(version)) => {
                found.node = Some(version.to_owned());
            }
            (Some("id"), Some(id), None) => found.machine_id = Some(id.to_owned()),
            _ => {}
        }
    }

    let order = |agent: &String| {
        HARNESSES
            .iter()
            .position(|harness| harness.agent == agent)
            .unwrap_or(HARNESSES.len())
    };
    found.agents.sort_by_key(order);
    found
}

#[cfg(test)]
#[path = "../../tests/machine_wsl_distro.rs"]
mod tests;
