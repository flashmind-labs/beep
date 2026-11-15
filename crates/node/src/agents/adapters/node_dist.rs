//! Which Node.js build the node downloads for its adapters: the newest LTS
//! from nodejs.org for this OS and architecture, checked against the
//! release's `SHASUMS256.txt`.

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::path::Path;

/// Official builds: glibc Linux, macOS and Windows.
const OFFICIAL: &str = "https://nodejs.org/dist";
/// Node's own builds for musl Linux, such as Alpine.
const MUSL: &str = "https://unofficial-builds.nodejs.org/download/release";
/// The oldest Node the adapters run on (claude-agent-acp needs 22).
const MIN_MAJOR: u64 = 22;

/// A Node.js build for one OS and architecture.
#[derive(Debug, PartialEq)]
pub(super) struct NodeDist {
    base: &'static str,
    /// As the file names say it, such as `linux-x64` or `win-arm64`.
    platform: &'static str,
    /// As `index.json` lists it in a release's `files`.
    index_key: &'static str,
    zip: bool,
}

/// The build for `os` and `arch`, as `std::env::consts` name them, on a
/// Linux whose C library is musl when `musl`. `None` where Node has none.
pub(super) fn node_dist(os: &str, arch: &str, musl: bool) -> Option<NodeDist> {
    let dist = |base, platform, index_key, zip| NodeDist {
        base,
        platform,
        index_key,
        zip,
    };
    Some(match (os, arch, musl) {
        ("linux", "x86_64", false) => dist(OFFICIAL, "linux-x64", "linux-x64", false),
        ("linux", "aarch64", false) => dist(OFFICIAL, "linux-arm64", "linux-arm64", false),
        ("linux", "x86_64", true) => dist(MUSL, "linux-x64-musl", "linux-x64-musl", false),
        ("linux", "aarch64", true) => dist(MUSL, "linux-arm64-musl", "linux-arm64-musl", false),
        ("macos", "aarch64", _) => dist(OFFICIAL, "darwin-arm64", "osx-arm64-tar", false),
        ("macos", "x86_64", _) => dist(OFFICIAL, "darwin-x64", "osx-x64-tar", false),
        ("windows", "x86_64", _) => dist(OFFICIAL, "win-x64", "win-x64-zip", true),
        ("windows", "aarch64", _) => dist(OFFICIAL, "win-arm64", "win-arm64-zip", true),
        _ => return None,
    })
}

impl NodeDist {
    /// The list of releases.
    pub(super) fn index_url(&self) -> String {
        format!("{}/index.json", self.base)
    }

    /// The archive of release `version`, such as `node-v24.1.0-linux-x64.tar.gz`.
    pub(super) fn archive(&self, version: &str) -> String {
        let ext = if self.zip { "zip" } else { "tar.gz" };
        format!("node-{version}-{}.{ext}", self.platform)
    }

    /// The folder the archive unpacks to.
    pub(super) fn folder(&self, version: &str) -> String {
        format!("node-{version}-{}", self.platform)
    }

    pub(super) fn url(&self, version: &str) -> String {
        format!("{}/{version}/{}", self.base, self.archive(version))
    }

    pub(super) fn sums_url(&self, version: &str) -> String {
        format!("{}/{version}/SHASUMS256.txt", self.base)
    }

    /// The newest LTS release in `index` (nodejs.org's `index.json`, newest
    /// first) that has this build.
    pub(super) fn newest_lts(&self, index: &str) -> Result<String> {
        #[derive(Deserialize)]
        struct Release {
            version: String,
            /// `false`, or the LTS line's name.
            lts: serde_json::Value,
            files: Vec<String>,
        }
        let releases: Vec<Release> =
            serde_json::from_str(index).context("reading Node.js's list of releases")?;
        let release = releases
            .into_iter()
            .find(|release| {
                release.lts.is_string() && release.files.iter().any(|f| f == self.index_key)
            })
            .with_context(|| format!("no Node.js LTS release for {}", self.platform))?;
        let version = release.version;
        let major = version
            .strip_prefix('v')
            .and_then(|rest| rest.split('.').next())
            .and_then(|major| major.parse::<u64>().ok());
        ensure!(
            major.is_some_and(|major| major >= MIN_MAJOR)
                && version
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.'),
            "{version} is not a Node.js version the adapters run on"
        );
        Ok(version)
    }
}

/// Whether this Linux's C library is musl, as on Alpine: musl's loader is
/// under `root` (`/` but in tests) and glibc's is not. A glibc system can
/// have musl installed beside it.
pub(super) fn is_musl(root: &Path) -> bool {
    let has = |dir: &str, prefix: &str| {
        std::fs::read_dir(root.join(dir)).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with(prefix))
            })
        })
    };
    let glibc = [
        "lib64",
        "lib",
        "lib/x86_64-linux-gnu",
        "lib/aarch64-linux-gnu",
        "usr/lib64",
    ]
    .iter()
    .any(|dir| has(dir, "ld-linux-"));
    !glibc && has("lib", "ld-musl-")
}

#[cfg(test)]
#[path = "../../tests/agents_adapters_node_dist.rs"]
mod tests;
