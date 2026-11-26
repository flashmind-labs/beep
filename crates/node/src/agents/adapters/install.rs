//! Installs an agent's ACP adapter in the node's folder: Node.js from
//! nodejs.org first when the node has none, then the adapter's npm package
//! with that Node's npm. Each is staged, then renamed into place whole.

use super::{
    layout::{self, Installed},
    node_dist::{self, NodeDist},
};
use crate::{
    agents::known::{Adapter, Known},
    machine::provision::{curl, listed_sha256, remove_dir, unpack, verify}, process::spawn_env,
    process::command,
};
use anyhow::{Context, Result, bail};
use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{task::spawn_blocking, time::timeout};

/// The longest one `npm install` may take.
const NPM_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// The longest unpacking Node.js or asking it its version may take.
const LOCAL_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Installs the adapter of `known` under the adapters' folder `root`, and
/// Node.js beside it when it is not there yet.
pub(in crate::agents) async fn adapter(root: &Path, known: &Known) -> Result<Installed> {
    let Adapter::Npm { bin, package, min } = known.adapter else {
        bail!("{} has no adapter to install", known.name);
    };
    let node = node(root).await.context("needs Node.js")?;
    let stage = layout::stage(root, known.name);
    remove_dir(&stage).await?;
    tokio::fs::create_dir_all(&stage)
        .await
        .with_context(|| format!("creating {}", stage.display()))?;
    let installed = npm_install(root, &node, &format!("{package}@>={min}"), &stage)
        .await
        .and_then(|()| {
            layout::script_in(&stage, package, bin)
                .map(|_| ())
                .with_context(|| format!("{package} installed no {bin}"))
        });
    if let Err(e) = installed {
        clean(&stage).await;
        return Err(e);
    }

    let target = layout::prefix(root, known.name);
    swap_in(&stage, &target).await?;
    let (script, _) = layout::script_in(&target, package, bin)
        .with_context(|| format!("{bin} is gone from {}", target.display()))?;
    Ok(Installed {
        node,
        script,
        current: true,
    })
}

/// The node's own `node`, downloaded when it is not there yet.
async fn node(root: &Path) -> Result<PathBuf> {
    let node_root = layout::node_root(root);
    let program = layout::node_program(&node_root, env::consts::OS);

    if program.is_file() {
        return Ok(program);
    }

    let musl = env::consts::OS == "linux" && node_dist::is_musl(Path::new("/"));
    let dist =
        node_dist::node_dist(env::consts::OS, env::consts::ARCH, musl).with_context(|| {
            format!(
                "Node.js has no build for {} {}",
                env::consts::OS,
                env::consts::ARCH
            )
        })?;
    let stage = layout::stage(root, "node");
    remove_dir(&stage).await?;
    tokio::fs::create_dir_all(&stage)
        .await
        .with_context(|| format!("creating {}", stage.display()))?;
    let unpacked = download_node(&dist, &stage).await;
    let unpacked = match unpacked {
        Ok(unpacked) => unpacked,
        Err(e) => {
            clean(&stage).await;
            return Err(e);
        }
    };
    swap_in(&unpacked, &node_root).await?;
    clean(&stage).await;
    Ok(program)
}

/// Downloads, checks and unpacks the newest LTS of `dist` into `stage`;
/// the folder it unpacked to, with a `node` that runs.
async fn download_node(dist: &NodeDist, stage: &Path) -> Result<PathBuf> {
    let index = curl(&dist.index_url(), None).await?;
    let index = String::from_utf8(index).context("reading Node.js's list of releases")?;
    let version = dist.newest_lts(&index)?;
    log::info!("downloading Node.js {version}");
    let archive = dist.archive(&version);
    let sums = curl(&dist.sums_url(&version), None).await?;
    let sums = String::from_utf8(sums).context("reading SHASUMS256.txt")?;
    let sha256 = listed_sha256(&sums, &archive)
        .with_context(|| format!("{archive} is not in SHASUMS256.txt"))?
        .to_owned();
    let download = stage.join(&archive);
    curl(&dist.url(&version), Some(&download)).await?;
    let file = download.clone();
    spawn_blocking(move || verify(&file, &sha256)).await??;
    unpack(&download, stage).await?;
    let unpacked = stage.join(dist.folder(&version));
    let program = layout::node_program(&unpacked, env::consts::OS);
    let mut version_of = command(&program);
    version_of.arg("--version");
    let out = run(&mut version_of, LOCAL_TIMEOUT)
        .await
        .context("the downloaded node does not run")?;
    log::info!("Node.js {} unpacked", String::from_utf8_lossy(&out).trim());
    Ok(unpacked)
}

/// Installs `package` under the npm prefix `prefix` with the npm of the
/// node's Node.js in the adapters' folder `root`; its `node` goes first on
/// `PATH` for the install scripts.
async fn npm_install(root: &Path, node: &Path, package: &str, prefix: &Path) -> Result<()> {
    let npm = layout::npm_script(&layout::node_root(root), env::consts::OS);
    let mut install = command(node);
    spawn_env::apply(&mut install, spawn_env::Scope::Probe);
    install
        .arg(&npm)
        .args([
            "install",
            "--no-audit",
            "--no-fund",
            "--omit=dev",
            "--loglevel=error",
            "--prefix",
        ])
        .arg(prefix)
        .arg(package)
        .env("PATH", path_with_node(node))
        .current_dir(prefix);
    run(&mut install, NPM_TIMEOUT)
        .await
        .with_context(|| format!("npm install {package}"))?;
    Ok(())
}

/// The `PATH` processes get, with `node`'s folder first.
fn path_with_node(node: &Path) -> OsString {
    let inherited = spawn_env::search_path().unwrap_or_default();
    let dirs = node
        .parent()
        .map(Path::to_owned)
        .into_iter()
        .chain(env::split_paths(&inherited));
    env::join_paths(dirs).unwrap_or(inherited)
}

/// Runs `command` to the end within `limit`; its output, or why it failed
/// with what it said on stderr.
async fn run(command: &mut tokio::process::Command, limit: Duration) -> Result<Vec<u8>> {
    let output = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output();
    let out = timeout(limit, output)
        .await
        .context("it took too long")?
        .context("it did not start")?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        let last: Vec<&str> = said.lines().rev().take(5).collect();
        let last: Vec<&str> = last.into_iter().rev().collect();
        bail!("{}: {}", out.status, last.join(" / "));
    }

    Ok(out.stdout)
}

/// Puts the finished `staged` folder at `target`, replacing what was
/// there, which was not whole or it would not be installed again.
async fn swap_in(staged: &Path, target: &Path) -> Result<()> {
    remove_dir(target).await?;
    tokio::fs::rename(staged, target)
        .await
        .with_context(|| format!("moving {} into place", target.display()))
}

/// Removes a stage folder, saying so when it cannot.
async fn clean(stage: &Path) {
    if let Err(e) = remove_dir(stage).await {
        log::warn!("could not clean up {}: {e:#}", stage.display());
    }
}
