//! Finding the newest release, downloading it and saying what a restart did.

use super::{
    archive::{unpack, verify},
    http::{fetch, url},
    install::{replace, this_binary, writable},
    target::Target,
    types::{Error, Version},
};
use crate::machine::service::Restart;
use beep_proto::node::update::ReleaseChannel;
use reqwest::Client;
use std::path::PathBuf;
use tokio::task::spawn_blocking;

/// This binary's version.
const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// This binary's version and the newest release's.
pub(super) struct Newest {
    pub(super) current: Version,
    pub(super) latest: Version,
}

pub(super) async fn newest(
    client: &Client,
    base: &str,
    channel: ReleaseChannel,
) -> Result<Newest, Error> {
    let current: Version = CURRENT.parse()?;
    let latest = fetch(client, &url(base, Into::<&str>::into(channel))?).await?;
    let latest: Version = String::from_utf8_lossy(&latest).parse()?;
    Ok(Newest { current, latest })
}

/// A verified release, not yet in place of this binary.
pub(super) struct Download {
    exe: PathBuf,
    binary: Vec<u8>,
}

/// Downloads `latest` and checks it against the release's `SHA256SUMS`.
pub(super) async fn download(
    client: &Client,
    base: &str,
    latest: &Version,
) -> Result<Download, Error> {
    let target = Target::current()?;
    let exe = this_binary()?;
    // Fails before the download when the binary cannot be replaced.
    let probe = exe.clone();
    spawn_blocking(move || writable(&probe)).await??;

    let archive = target.archive(latest);
    let packed = fetch(client, &url(base, &format!("{latest}/{archive}"))?).await?;
    let sums = fetch(client, &url(base, &format!("{latest}/SHA256SUMS"))?).await?;
    let binary = spawn_blocking(move || {
        verify(&archive, &packed, &String::from_utf8_lossy(&sums))?;
        unpack(target, &archive, &packed)
    })
    .await??;
    Ok(Download { exe, binary })
}

impl Download {
    /// Puts it in place of this binary, whose path it gives.
    pub(super) async fn install(self) -> Result<PathBuf, Error> {
        let Download { exe, binary } = self;
        let installed = exe.clone();
        spawn_blocking(move || replace(&installed, &binary)).await??;
        Ok(exe)
    }
}

/// What restarting did, in words for the user.
pub(super) fn said(restart: &Restart, latest: &Version) -> String {
    match restart {
        Restart::Restarted(_) => format!("Restarted the beep-node service on {latest}."),
        Restart::Stopped(manager) => format!(
            "The beep-node service is stopped, so it was left so. Start it on {latest} with:\n  {}",
            manager.start_command()
        ),
        Restart::OtherBinary(file) => format!(
            "The beep-node service ({}) runs another binary, so it was not restarted.",
            file.display()
        ),
        Restart::NotInstalled => "beep-node is not installed as a service here, so nothing was \
             restarted. A beep-node still running keeps the old version until it restarts."
            .to_owned(),
    }
}
