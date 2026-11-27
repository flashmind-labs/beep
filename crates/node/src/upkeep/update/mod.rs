//! `beep-node update` (`update`): replaces this binary with the newest
//! release, checked against its `SHA256SUMS`, and restarts the service
//! running it, as rerunning `install.sh` or `install.ps1` does.

mod archive;
mod http;
mod install;
pub mod reexec;
mod release;
mod remote;
mod target;
mod types;

use crate::{
    upkeep::restart::{self, Drained},
    machine::service,
};
use beep_proto::node::update::ReleaseChannel;
use http::{base, client};
use install::this_binary;
use release::{Newest, download, newest, said};
use std::{cmp::Ordering, path::{Path, PathBuf}, time::Duration};
use tokio::task::spawn_blocking;

pub use remote::{Updates, WAIT_MINUTES};
pub use target::Target;
pub use types::{Error, Version};


/// `beep-node update`: installs the newest release over this binary and
/// restarts its service. With `check`, only says whether there is one.
/// Otherwise it waits up to `wait` for the node's agent sessions to be
/// idle before restarting it, or restarts at once when `wait` is `None`;
/// `dir` is the node's folder, where the running node is asked to wait.
pub async fn update(check: bool, wait: Option<Duration>, dir: PathBuf) -> Result<(), Error> {
    let base = base();
    let client = client()?;
    let Newest { current, latest } = newest(&client, &base, ReleaseChannel::All).await?;

    match current.cmp(&latest) {
        // The binary is the newest, but the node may still run an older one,
        // as when an update waits for its agent sessions to finish.
        Ordering::Equal => {
            return match running(&dir).await {
                Some(running) if running < current && check => {
                    println!(
                        "beep-node {current} is installed, but the node still runs {running}. \
                         Run `beep-node update` to restart it."
                    );
                    Ok(())
                }
                Some(running) if running < current => {
                    println!("beep-node {current} is installed; restarting the node from {running}");
                    let connection = idle(wait, &dir).await;
                    restarted(this_binary()?, connection, &latest).await
                }
                _ => {
                    println!("beep-node {current} is the newest");
                    Ok(())
                }
            };
        }
        Ordering::Greater => {
            println!("beep-node {current} is newer than the latest release, {latest}");
            return Ok(());
        }
        Ordering::Less if check => {
            println!("beep-node {latest} is out; this is {current}. Run `beep-node update`.");
            return Ok(());
        }
        Ordering::Less => {}
    }

    println!("Downloading beep-node {latest} for {}", Target::current()?);
    let download = download(&client, &base, &latest).await?;
    let connection = idle(wait, &dir).await;
    let exe = download.install().await?;
    println!("Installed {}", exe.display());

    restarted(exe, connection, &latest).await
}

/// Waits for the agent sessions to be idle, up to `wait`. The connection is
/// held until the restart.
async fn idle(wait: Option<Duration>, dir: &Path) -> Option<restart::UpdateConnection> {
    match wait {
        Some(limit) => wait_for_idle(dir, limit).await,
        None => None,
    }
}

/// Restarts the service on `exe` and says what happened.
async fn restarted(
    exe: PathBuf,
    _connection: Option<restart::UpdateConnection>,
    latest: &Version,
) -> Result<(), Error> {
    let restarted = {
        let exe = exe.clone();
        spawn_blocking(move || service::restart(&exe)).await?
    };
    let restarted = restarted.map_err(|source| Error::Restart {
        version: latest.clone(),
        source,
    })?;
    println!("{}", said(&restarted, latest));
    Ok(())
}

/// The version of the node running from `dir`, if one runs and says it.
#[cfg(feature = "cli")]
async fn running(dir: &Path) -> Option<Version> {
    let status = crate::upkeep::local_status(dir).await.ok()??;
    status.version.parse().ok()
}

/// Only the CLI reads the running node.
#[cfg(not(feature = "cli"))]
async fn running(_dir: &Path) -> Option<Version> {
    None
}

/// Asks the running node to finish its agents' turns,
/// waiting up to `limit`. Returns the connection while the update continues.
async fn wait_for_idle(dir: &Path, limit: Duration) -> Option<restart::UpdateConnection> {
    println!(
        "Waiting up to {} min for the agent sessions to finish their turns \
         (`--now` skips this); new prompts can still start",
        limit.as_secs().div_ceil(60)
    );

    match restart::drain(dir, limit).await {
        Drained::NoNode => None,
        Drained::Idle(connection) => Some(connection),
        Drained::TimedOut(connection) => {
            println!(
                "A turn still runs; restarting anyway. Its session resumes after the restart."
            );
            Some(connection)
        }
    }
}

#[cfg(test)]
#[path = "../../tests/upkeep_update.rs"]
mod tests;
