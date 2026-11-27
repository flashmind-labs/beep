//! A WSL distribution's name, and what `wsl.exe` and the probe inside a
//! distribution print.

use crate::machine::service::from_utf16;
use beep_proto::node::harness::HARNESSES;
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsStr,
    fmt,
    path::{Path, PathBuf},
    str::FromStr,
};

/// A WSL distribution's name, such as `Ubuntu-24.04`: letters, digits, `.`,
/// `_` and `-`, the names `wsl.exe` registers.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DistroName(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
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

/// A distribution as `wsl.exe --list` gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Listed {
    pub(crate) name: DistroName,
    pub(crate) running: bool,
    /// The one `wsl.exe` starts by default, marked `*`.
    pub(crate) default: bool,
}

/// The distributions in what `wsl.exe --list --verbose` printed: a header,
/// then a line each of name, state and WSL version, the default one marked
/// `*`. `None` when a state is not one this reads, as on a Windows in
/// another language.
pub(crate) fn listed(verbose: &str) -> Option<Vec<Listed>> {
    let mut found = Vec::new();
    let lines = verbose
        .lines()
        .map(|line| line.trim_matches(|c: char| c.is_whitespace() || c == '\0'))
        .filter(|line| !line.is_empty())
        .skip(1);

    for line in lines {
        let default = line.starts_with('*');
        let words: Vec<&str> = line.trim_start_matches('*').split_whitespace().collect();
        let [name, state @ .., _version] = words.as_slice() else {
            return None;
        };
        let running = match state.join(" ").as_str() {
            "Running" => true,
            "Stopped" | "Installing" | "Converting" | "Uninstalling" => false,
            _ => return None,
        };

        if let Ok(name) = name.parse::<DistroName>()
            && !for_an_app(&name)
        {
            found.push(Listed {
                name,
                running,
                default,
            });
        }
    }

    Some(found)
}

/// A distribution an app runs its own engine in, such as
/// `docker-desktop-data`, which has no user to run agents for.
fn for_an_app(name: &DistroName) -> bool {
    ["docker-desktop", "rancher-desktop"]
        .iter()
        .any(|app| name.0.starts_with(app))
}

/// An absolute path inside a WSL distribution, such as `/home/sam`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxPath(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not an absolute path in WSL")]
pub struct NotLinuxPath(String);

impl FromStr for LinuxPath {
    type Err = NotLinuxPath;

    fn from_str(path: &str) -> Result<Self, Self::Err> {
        match path.starts_with('/') {
            true => Ok(Self(path.to_owned())),
            false => Err(NotLinuxPath(path.to_owned())),
        }
    }
}

impl LinuxPath {
    /// `tail`, a relative path, inside this one.
    pub(crate) fn join(&self, tail: &str) -> Self {
        Self(format!("{}/{tail}", self.0.trim_end_matches('/')))
    }
}

impl fmt::Display for LinuxPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for LinuxPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// `path` on a Windows drive as a distribution sees it, such as
/// `C:\Users\sam` as `/mnt/c/Users/sam`; `None` for a path on no drive
/// letter, such as a network share.
pub(crate) fn linux_path(path: &Path) -> Option<LinuxPath> {
    let path = path.to_string_lossy();
    let path = path.strip_prefix(r"\\?\").unwrap_or(&path);
    let mut chars = path.chars();
    let (Some(drive), Some(':')) = (chars.next(), chars.next()) else {
        return None;
    };
    let rest = chars.as_str();

    if !drive.is_ascii_alphabetic() || !(rest.is_empty() || rest.starts_with(['\\', '/'])) {
        return None;
    }

    let rest = rest.replace('\\', "/");
    let rest = rest.trim_end_matches('/');
    Some(LinuxPath(format!(
        "/mnt/{}{rest}",
        drive.to_ascii_lowercase()
    )))
}

/// A path on a mounted drive inside a distribution as Windows names it,
/// such as `/mnt/c/Users/sam` as `C:\Users\sam`; `None` for any other.
pub(crate) fn windows_drive_path(path: &str) -> Option<String> {
    let rest = path.strip_prefix("/mnt/")?;
    let mut chars = rest.chars();
    let drive = chars.next().filter(char::is_ascii_alphabetic)?;
    let rest = chars.as_str();

    if !(rest.is_empty() || rest.starts_with('/')) {
        return None;
    }

    let rest = match rest {
        "" => "\\".to_owned(),
        rest => rest.replace('/', "\\"),
    };
    Some(format!("{}:{rest}", drive.to_ascii_uppercase()))
}

/// The folder `path` inside `distro` as Windows names it: by its drive
/// when it is on one, else through `\\wsl$`.
pub(crate) fn windows_folder(distro: &DistroName, path: &str) -> PathBuf {
    match (windows_drive_path(path), path.parse::<LinuxPath>()) {
        (Some(path), _) => PathBuf::from(path),
        (None, Ok(linux)) => windows_path(distro, &linux),
        (None, Err(_)) => PathBuf::from(path),
    }
}

/// `path` inside `distro` as Windows reaches it, through `\\wsl$`, which
/// Windows 10 has too.
pub(crate) fn windows_path(distro: &DistroName, path: &LinuxPath) -> PathBuf {
    format!(r"\\wsl$\{distro}{}", path.0.replace('/', "\\")).into()
}

/// A coding agent CLI the probe found inside a distribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FoundAgent {
    /// Its name among the harnesses, such as `claude`.
    pub(crate) name: String,
    /// Where it is installed, inside the distribution.
    pub(crate) path: LinuxPath,
}

/// What the probe found inside a distribution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Found {
    /// Its default user's home.
    pub(crate) home: Option<LinuxPath>,
    /// The coding agent CLIs, in the order Beep lists harnesses.
    pub(crate) agents: Vec<FoundAgent>,
    /// The ACP adapters and `npx`.
    pub(crate) tools: Vec<FoundAgent>,
}

/// What the probe printed: `home <path>`, then `agent <name> <path>` for
/// each agent and `tool <name> <path>` for each adapter on the `PATH`.
/// Programs under `/mnt/` are Windows installs seen through the
/// `PATH` WSL appends, and do not count.
pub(crate) fn found(printed: &str) -> Found {
    let mut found = Found::default();

    for line in printed.lines() {
        let mut words = line.split_whitespace();

        match (words.next(), words.next(), words.next()) {
            (Some("home"), Some(home), None) => found.home = home.parse().ok(),
            (Some(kind @ ("agent" | "tool")), Some(name), Some(path))
                if !path.starts_with("/mnt/") =>
            {
                let list = match kind {
                    "agent" => &mut found.agents,
                    _ => &mut found.tools,
                };

                if let Ok(path) = path.parse::<LinuxPath>()
                    && !list.iter().any(|known| known.name == name)
                {
                    list.push(FoundAgent {
                        name: name.to_owned(),
                        path,
                    });
                }
            }
            _ => {}
        }
    }

    let order = |agent: &FoundAgent| {
        HARNESSES
            .iter()
            .position(|harness| harness.agent == agent.name)
            .unwrap_or(HARNESSES.len())
    };
    found.agents.sort_by_key(order);
    found
}

#[cfg(test)]
#[path = "../../tests/machine_wsl_distro.rs"]
mod tests;
