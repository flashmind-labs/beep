//! Where the adapters the node installs live in its folder:
//!
//! - `agent-adapters/node`: the Node.js the node downloaded.
//! - `agent-adapters/<agent>`: an npm prefix holding that agent's adapter.
//! - `agent-adapters/.stage-<what>`: an install on its way, renamed into
//!   place only once it is complete.

use crate::agents::known::{Adapter, Known};
use semver::Version;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Component, Path, PathBuf},
};

/// The folder in the node's own folder that holds them.
const DIR: &str = "agent-adapters";

/// An adapter the node installed: started as `node script`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Installed {
    pub(crate) node: PathBuf,
    pub(crate) script: PathBuf,
    /// At least the version the node asks for: an older one is installed
    /// again.
    pub(crate) current: bool,
}

/// The adapters' folder in the node's folder `node_dir`.
pub(in crate::agents) fn root(node_dir: &Path) -> PathBuf {
    node_dir.join(DIR)
}

/// Where the node's Node.js is unpacked.
pub(super) fn node_root(root: &Path) -> PathBuf {
    root.join("node")
}

/// The `node` program of a Node.js unpacked at `node_root` for `os`.
pub(super) fn node_program(node_root: &Path, os: &str) -> PathBuf {
    if os == "windows" {
        node_root.join("node.exe")
    } else {
        node_root.join("bin/node")
    }
}

/// npm's own script in a Node.js unpacked at `node_root` for `os`, run with
/// its `node` so no `npm.cmd` or shebang is needed.
pub(super) fn npm_script(node_root: &Path, os: &str) -> PathBuf {
    if os == "windows" {
        node_root.join("node_modules/npm/bin/npm-cli.js")
    } else {
        node_root.join("lib/node_modules/npm/bin/npm-cli.js")
    }
}

/// The npm prefix that holds `agent`'s adapter.
pub(super) fn prefix(root: &Path, agent: &str) -> PathBuf {
    root.join(agent)
}

/// The folder an install of `what` is staged in.
pub(super) fn stage(root: &Path, what: &str) -> PathBuf {
    root.join(format!(".stage-{what}"))
}

/// The adapter of `known` the node installed in `node_dir`, when it is
/// there whole: its Node and its script.
pub(crate) fn installed(node_dir: &Path, known: &Known) -> Option<Installed> {
    let Adapter::Npm { bin, package, min } = known.adapter else {
        return None;
    };
    let root = root(node_dir);
    let node = node_program(&node_root(&root), env::consts::OS);

    if !node.is_file() {
        return None;
    }

    let (script, version) = script_in(&prefix(&root, known.name), package, bin)?;
    let current = Version::parse(min).is_ok_and(|min| version.is_some_and(|v| v >= min));
    Some(Installed {
        node,
        script,
        current,
    })
}

/// The script that runs `bin` of `package`, installed under `prefix`, and
/// the package's version when it says one.
pub(super) fn script_in(
    prefix: &Path,
    package: &str,
    bin: &str,
) -> Option<(PathBuf, Option<Version>)> {
    let dir = package_dir(prefix, package);
    let manifest = fs::read_to_string(dir.join("package.json")).ok()?;
    let script = dir.join(bin_script(&manifest, bin)?);
    script.is_file().then(|| (script, version(&manifest)))
}

/// The `version` a `package.json` gives, when it is one semver reads.
fn version(manifest: &str) -> Option<Version> {
    #[derive(Deserialize)]
    struct Versioned {
        version: String,
    }

    let manifest: Versioned = serde_json::from_str(manifest).ok()?;
    Version::parse(&manifest.version).ok()
}

/// Where npm puts `package` under `prefix`; a scoped name is two folders.
fn package_dir(prefix: &Path, package: &str) -> PathBuf {
    package
        .split('/')
        .fold(prefix.join("node_modules"), |dir, part| dir.join(part))
}

/// The `bin` field of a `package.json`: one script named after the
/// package, or scripts by name.
#[derive(Deserialize)]
#[serde(untagged)]
enum Bins {
    One(String),
    Named(BTreeMap<String, String>),
}

#[derive(Deserialize)]
struct Manifest {
    name: String,
    bin: Option<Bins>,
}

/// The script, relative to the package, that its `package.json` names for
/// `bin`. `None` when it names none, or one outside the package.
pub(super) fn bin_script(manifest: &str, bin: &str) -> Option<PathBuf> {
    let manifest: Manifest = serde_json::from_str(manifest).ok()?;
    let script = match manifest.bin? {
        Bins::One(script) => {
            let unscoped = manifest.name.rsplit('/').next().unwrap_or_default();
            (unscoped == bin).then_some(script)?
        }
        Bins::Named(mut scripts) => scripts.remove(bin)?,
    };
    let script = PathBuf::from(script);
    script
        .components()
        .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
        .then_some(script)
}

#[cfg(test)]
#[path = "../../tests/agents_adapters_layout.rs"]
mod tests;
