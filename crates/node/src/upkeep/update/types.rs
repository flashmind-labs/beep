//! What the update shares: why it stopped (`Error`) and a release's version
//! (`Version`).

use reqwest::{StatusCode, Url};
use std::{cmp::Ordering, fmt, io, path::PathBuf, str::FromStr};
use tokio::task::JoinError;

/// Why the update stopped.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("bad version {0:?}")]
    BadVersion(String),
    #[error("bad download location {text:?}")]
    BadBase {
        text: String,
        #[source]
        source: <Url as FromStr>::Err,
    },
    #[error("could not set up downloads")]
    Client(#[source] reqwest::Error),
    #[error("could not reach {url}; is this machine online?")]
    Network {
        url: Url,
        #[source]
        source: reqwest::Error,
    },
    #[error("{url} answered {status}")]
    Status { url: Url, status: StatusCode },
    #[error("there is no beep-node build for {os} on {arch}")]
    Platform {
        os: &'static str,
        arch: &'static str,
    },
    #[error("{archive} is not listed in SHA256SUMS")]
    NotListed { archive: String },
    #[error(
        "checksum mismatch for {archive} (expected {expected}, got {actual}); nothing was installed"
    )]
    Checksum {
        archive: String,
        expected: String,
        actual: String,
    },
    #[error("could not unpack {archive}")]
    Unpack {
        archive: String,
        #[source]
        source: io::Error,
    },
    #[error("{archive} holds no {binary}")]
    Missing {
        archive: String,
        binary: &'static str,
    },
    #[error(
        "cannot write to {}; rerun `beep-node update` as the user that owns it, \
         or with sudo if it is a system folder",
        dir.display()
    )]
    NoPermission { dir: PathBuf },
    #[error("{action} {}", path.display())]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("installed beep-node {version}, but could not restart its service")]
    Restart {
        version: Version,
        #[source]
        source: anyhow::Error,
    },
    #[error("the update stopped")]
    Join(#[from] JoinError),
}

/// A release's version: dot-separated numbers, compared as numbers, so
/// `0.1.10` is newer than `0.1.9` and `1.2` is `1.2.0`.
#[derive(Debug, Clone)]
pub struct Version {
    text: String,
    /// The numbers without trailing zeros.
    parts: Vec<u64>,
}

impl FromStr for Version {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self, Error> {
        let bad = || Error::BadVersion(text.to_owned());
        let trimmed = text.trim();
        let trimmed = trimmed.strip_prefix('v').unwrap_or(trimmed);
        let mut parts = trimmed
            .split('.')
            .map(|part| {
                if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(bad());
                }

                part.parse::<u64>().map_err(|_| bad())
            })
            .collect::<Result<Vec<_>, _>>()?;
        while parts.last() == Some(&0) {
            parts.pop();
        }

        Ok(Self {
            text: trimmed.to_owned(),
            parts,
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.parts == other.parts
    }
}

impl Eq for Version {}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.parts.cmp(&other.parts)
    }
}

#[cfg(test)]
#[path = "../../tests/upkeep_update_version.rs"]
mod tests;
