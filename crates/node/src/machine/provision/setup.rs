//! Installs what the agents need and the machine lacks, for `setup/1`:
//! Node.js into the node's own folder, so npm installs the coding agents
//! without an admin password, and Git from the system's own installer.

use super::tools::{curl, listed_sha256, remove_dir, unpack, verify};
use crate::machine::spawn_env::{self, Scope};
use anyhow::{Context, Result, bail, ensure};
use beep_proto::node::setup::Reply;
use serde::Deserialize;
use std::{
    env,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::time::timeout;

const NODE_DIST: &str = "https://nodejs.org/dist";
/// The longest a system installer may take.
const INSTALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Where the node keeps the Node.js it installed.
fn node_home(dir: &Path) -> PathBuf {
    dir.join("nodejs")
}

/// The folders the setup installed into that processes need on `PATH`: the
/// node's Node.js, with the commands npm installs globally, and Git for
/// Windows, which winget does not put on the running node's `PATH`.
pub(super) fn path_dirs(dir: &Path) -> Vec<PathBuf> {
    let node = node_home(dir);
    let node = if cfg!(windows) { node } else { node.join("bin") };
    let git = cfg!(windows)
        .then(|| env::var_os("ProgramFiles"))
        .flatten()
        .map(|programs| PathBuf::from(programs).join("Git").join("cmd"));
    [Some(node), git]
        .into_iter()
        .flatten()
        .filter(|dir| dir.is_dir())
        .collect()
}

/// One release in Node.js's `index.json`, newest first there.
#[derive(Deserialize)]
struct NodeRelease {
    version: String,
    /// The LTS line's name, or `false` for a current release.
    lts: serde_json::Value,
}

/// Installs the newest Node.js LTS release into the node's folder `dir`,
/// checked against the release's published checksums.
pub(super) async fn install_node(dir: &Path) -> Result<()> {
    let (platform, ext) = node_platform(env::consts::OS, env::consts::ARCH).with_context(|| {
        format!(
            "Node.js has no release for {} {}",
            env::consts::OS,
            env::consts::ARCH
        )
    })?;
    let index = curl(&format!("{NODE_DIST}/index.json"), None).await?;
    let releases: Vec<NodeRelease> =
        serde_json::from_slice(&index).context("reading the Node.js releases")?;
    let version = releases
        .into_iter()
        .find(|release| release.lts.is_string())
        .map(|release| release.version)
        .context("Node.js lists no LTS release")?;
    ensure!(
        version.starts_with('v')
            && version
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.'),
        "{version} is not a Node.js version"
    );

    let stem = format!("node-{version}-{platform}");
    let work = dir.join(".download-nodejs");
    // A download left from a stopped node starts over.
    remove_dir(&work).await?;
    tokio::fs::create_dir_all(&work)
        .await
        .with_context(|| format!("creating {}", work.display()))?;
    let installed = unpack_node(dir, &work, &version, &stem, ext).await;

    if let Err(e) = remove_dir(&work).await {
        log::warn!("could not clean up {}: {e:#}", work.display());
    }

    installed
}

async fn unpack_node(dir: &Path, work: &Path, version: &str, stem: &str, ext: &str) -> Result<()> {
    let base = format!("{NODE_DIST}/{version}");
    let file = format!("{stem}.{ext}");
    let sums = String::from_utf8(curl(&format!("{base}/SHASUMS256.txt"), None).await?)
        .context("reading the Node.js checksums")?;
    let sha256 = listed_sha256(&sums, &file)
        .with_context(|| format!("{file} is not in the Node.js checksums"))?
        .to_owned();

    let archive = work.join(&file);
    curl(&format!("{base}/{file}"), Some(&archive)).await?;
    let check = archive.clone();
    tokio::task::spawn_blocking(move || verify(&check, &sha256)).await??;

    unpack(&archive, work).await?;

    let home = node_home(dir);
    remove_dir(&home).await?;
    tokio::fs::rename(work.join(stem), &home)
        .await
        .context("moving Node.js into place")
}

/// How Node.js names the release for `os` and `arch`, and its archive's
/// extension.
fn node_platform(os: &str, arch: &str) -> Option<(String, &'static str)> {
    let (os, ext) = match os {
        "linux" => ("linux", "tar.gz"),
        "macos" => ("darwin", "tar.gz"),
        "windows" => ("win", "zip"),
        _ => return None,
    };
    let arch = match arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => return None,
    };
    Some((format!("{os}-{arch}"), ext))
}

/// Installs Git: with Homebrew or the Xcode tools on macOS, winget on
/// Windows. Elsewhere the system's package manager needs an admin.
pub(super) async fn install_git() -> Result<Reply> {
    match env::consts::OS {
        "macos" => match brew() {
            Some(brew) => {
                let mut command = crate::run::process::command(brew);
                command.args(["install", "git"]);
                run(command, "brew install git").await?;
                Ok(Reply::Installed {})
            }
            None => {
                let mut command = crate::run::process::command("xcode-select");
                command.arg("--install");
                run(command, "xcode-select --install").await?;
                Ok(Reply::Started {})
            }
        },
        "windows" => {
            let mut command = crate::run::process::command("winget");
            command.args([
                "install",
                "--id",
                "Git.Git",
                "--exact",
                "--source",
                "winget",
                "--silent",
                "--accept-package-agreements",
                "--accept-source-agreements",
            ]);
            run(command, "winget install Git.Git").await?;
            Ok(Reply::Installed {})
        }
        _ => bail!(
            "Install Git with the system's package manager, such as `sudo apt install git`, then check again"
        ),
    }
}

/// Homebrew, on `PATH` or where its installer puts it.
fn brew() -> Option<PathBuf> {
    which::which_in("brew", spawn_env::search_path(), "/")
        .ok()
        .or_else(|| {
            ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"]
                .map(PathBuf::from)
                .into_iter()
                .find(|brew| brew.is_file())
        })
}

/// Runs `command` to the end; fails with its output unless it succeeds.
async fn run(mut command: tokio::process::Command, what: &str) -> Result<()> {
    spawn_env::apply(&mut command, Scope::Probe);
    let out = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output();
    let out = timeout(INSTALL_TIMEOUT, out)
        .await
        .with_context(|| format!("{what} took too long"))?
        .with_context(|| format!("starting {what}"))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let said = [stderr.trim(), stdout.trim()]
            .into_iter()
            .find(|text| !text.is_empty())
            .unwrap_or("no output");
        bail!("{what} failed: {said}");
    }

    Ok(())
}
