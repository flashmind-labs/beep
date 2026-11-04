//! `beep-node update`: replaces this binary with the newest release and
//! restarts the service running it. It does what rerunning `install.sh` or
//! `install.ps1` does on an upgrade, for the binary that is running wherever
//! it lives: the same archives, checked against the release's `SHA256SUMS`.

mod archive;
mod http;
mod install;
mod remote;
mod target;
mod version;

use crate::{
    restart::{self, Drained},
    service::{self, Restart},
};
use reqwest::{Client, StatusCode, Url};
use std::{
    cmp::Ordering,
    env, io,
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};
use tokio::task::{JoinError, spawn_blocking};

pub use remote::{Updates, WAIT_MINUTES};
pub use target::Target;
pub use version::Version;

use archive::{unpack, verify};
use http::{base, client, fetch, url};
use install::{replace, writable};

/// This binary's version.
const CURRENT: &str = env!("CARGO_PKG_VERSION");

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
    #[error("could not set up HTTPS")]
    Tls(#[source] rustls::Error),
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
    #[error("beep-node runs on Macs with Apple silicon; there is no build for Intel Macs")]
    IntelMac,
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

/// How `beep-node update` runs.
#[derive(Debug, Clone)]
pub struct Options {
    /// Only says whether a newer version is out.
    pub check: bool,
    /// How long to wait for the node's agent sessions to be idle before
    /// restarting it; `None` restarts at once.
    pub wait: Option<Duration>,
    /// The node's folder, where the running node is asked to wait.
    pub dir: PathBuf,
}

/// `beep-node update`: installs the newest release over this binary and
/// restarts its service. With `check`, only says whether there is one.
pub async fn update(options: Options) -> Result<(), Error> {
    let Options { check, wait, dir } = options;
    let base = base();
    let client = client()?;
    let Newest { current, latest } = newest(&client, &base).await?;
    match current.cmp(&latest) {
        Ordering::Equal => {
            println!("beep-node {current} is the newest");
            return Ok(());
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
    println!(
        "Downloading beep-node {latest} for {}",
        Target::current()?.triple()
    );
    let download = download(&client, &base, &latest).await?;
    // Kept until the restart, so the node holds new prompts until then.
    let _holding = match wait {
        Some(limit) => wait_for_idle(&dir, limit).await,
        None => None,
    };
    let exe = download.install().await?;
    println!("Installed {}", exe.display());

    let restarted = {
        let exe = exe.clone();
        spawn_blocking(move || service::restart(&exe)).await?
    };
    let restarted = restarted.map_err(|source| Error::Restart {
        version: latest.clone(),
        source,
    })?;
    println!("{}", said(&restarted, &latest));
    Ok(())
}

/// What restarting did, in words for the user.
fn said(restart: &Restart, latest: &Version) -> String {
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

/// This binary's version and the newest release's.
struct Newest {
    current: Version,
    latest: Version,
}

async fn newest(client: &Client, base: &str) -> Result<Newest, Error> {
    let current: Version = CURRENT.parse()?;
    let latest = fetch(client, &url(base, "latest.txt")?).await?;
    let latest: Version = String::from_utf8_lossy(&latest).parse()?;
    Ok(Newest { current, latest })
}

/// A verified release, not yet in place of this binary.
struct Download {
    exe: PathBuf,
    binary: Vec<u8>,
}

/// Downloads `latest` and checks it against the release's `SHA256SUMS`.
async fn download(client: &Client, base: &str, latest: &Version) -> Result<Download, Error> {
    let target = Target::current()?;
    let exe = env::current_exe()
        .and_then(|exe| exe.canonicalize())
        .map_err(|source| Error::Io {
            action: "finding",
            path: PathBuf::from("this binary"),
            source,
        })?;
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
    async fn install(self) -> Result<PathBuf, Error> {
        let Download { exe, binary } = self;
        let installed = exe.clone();
        spawn_blocking(move || replace(&installed, &binary)).await??;
        Ok(exe)
    }
}

/// Asks the running node to finish its agents' turns and hold new prompts,
/// waiting up to `limit`. What holds them, while the node does.
async fn wait_for_idle(dir: &Path, limit: Duration) -> Option<restart::Holding> {
    println!(
        "Waiting up to {} min for the agent sessions to finish their turns \
         (`--now` skips this); new prompts wait until the restart",
        limit.as_secs().div_ceil(60)
    );
    match restart::drain(dir, limit).await {
        Drained::NoNode => None,
        Drained::Idle(holding) => Some(holding),
        Drained::TimedOut(holding) => {
            println!(
                "A turn still runs; restarting anyway. Its session resumes after the restart."
            );
            Some(holding)
        }
    }
}
