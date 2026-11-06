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

    pub(super) fn is_zip(&self) -> bool {
        self.zip
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
mod tests {
    use super::*;

    #[test]
    fn each_machine_gets_its_own_build() {
        let linux = node_dist("linux", "x86_64", false).expect("a build");
        assert_eq!(
            linux.url("v24.11.1"),
            "https://nodejs.org/dist/v24.11.1/node-v24.11.1-linux-x64.tar.gz"
        );
        assert_eq!(
            linux.sums_url("v24.11.1"),
            "https://nodejs.org/dist/v24.11.1/SHASUMS256.txt"
        );
        assert_eq!(linux.folder("v24.11.1"), "node-v24.11.1-linux-x64");
        let alpine = node_dist("linux", "aarch64", true).expect("a build");
        assert_eq!(
            alpine.url("v24.11.1"),
            "https://unofficial-builds.nodejs.org/download/release/v24.11.1/node-v24.11.1-linux-arm64-musl.tar.gz"
        );
        let mac = node_dist("macos", "aarch64", false).expect("a build");
        assert_eq!(mac.archive("v24.11.1"), "node-v24.11.1-darwin-arm64.tar.gz");
        let windows = node_dist("windows", "x86_64", false).expect("a build");
        assert!(windows.is_zip());
        assert_eq!(windows.archive("v24.11.1"), "node-v24.11.1-win-x64.zip");
        assert_eq!(node_dist("freebsd", "x86_64", false), None);
        assert_eq!(node_dist("linux", "riscv64", false), None);
    }

    #[test]
    fn the_newest_lts_with_this_build_is_picked() {
        let index = r#"[
            {"version":"v27.1.0","lts":false,"files":["linux-x64","win-x64-zip"]},
            {"version":"v26.2.0","lts":"Lithium","files":["linux-x64"]},
            {"version":"v24.11.1","lts":"Krypton","files":["linux-x64","win-x64-zip"]},
            {"version":"v20.19.0","lts":"Iron","files":["linux-x64","win-x64-zip"]}
        ]"#;
        let linux = node_dist("linux", "x86_64", false).expect("a build");
        assert_eq!(linux.newest_lts(index).expect("a release"), "v26.2.0");
        let windows = node_dist("windows", "x86_64", false).expect("a build");
        assert_eq!(windows.newest_lts(index).expect("a release"), "v24.11.1");
        let mac = node_dist("macos", "aarch64", false).expect("a build");
        assert!(mac.newest_lts(index).is_err());
        let old = r#"[{"version":"v20.19.0","lts":"Iron","files":["linux-x64"]}]"#;
        assert!(linux.newest_lts(old).is_err(), "too old for the adapters");
        let odd = r#"[{"version":"v24.1.0/../x","lts":"Krypton","files":["linux-x64"]}]"#;
        assert!(linux.newest_lts(odd).is_err());
    }

    #[test]
    fn musl_is_told_by_its_loader() {
        let alpine = tempfile::tempdir().expect("a temp folder");
        assert!(!is_musl(alpine.path()));
        std::fs::create_dir_all(alpine.path().join("lib")).expect("lib");
        std::fs::write(alpine.path().join("lib/ld-musl-x86_64.so.1"), "").expect("musl");
        assert!(is_musl(alpine.path()));
        // Ubuntu with the musl package: glibc's loader says glibc.
        let ubuntu = tempfile::tempdir().expect("a temp folder");
        for dir in ["lib", "lib64"] {
            std::fs::create_dir_all(ubuntu.path().join(dir)).expect("folders");
        }
        std::fs::write(ubuntu.path().join("lib/ld-musl-x86_64.so.1"), "").expect("musl");
        std::fs::write(ubuntu.path().join("lib64/ld-linux-x86-64.so.2"), "").expect("glibc");
        assert!(!is_musl(ubuntu.path()));
    }
}
