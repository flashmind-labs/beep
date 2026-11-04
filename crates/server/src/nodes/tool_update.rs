//! Newer `gh`, `teacli` and `flsh` for the machines: the newest release of
//! each, looked up every few hours, and installing it on a node over
//! `tool-update/1` (see `beep_proto::node::tool_update`).

use super::Online;
use crate::AsyncMutex;
use crate::protocol::node::tool_update::{FLSH, PROTOCOL, Releases, Reply, Request, Tool};
use anyhow::{Context, Result, bail};
use reqwest::{Client, header::USER_AGENT};
use serde::Deserialize;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio::time::timeout;

/// How long a found release is trusted before it is looked up again.
const FRESH: Duration = Duration::from_secs(6 * 60 * 60);
/// How long a failed lookup waits before the next try.
const RETRY: Duration = Duration::from_secs(15 * 60);
/// How long one lookup may take.
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(10);
/// How long the node has to download, check and install the release.
const INSTALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// The newest release of each tool, as last looked up.
#[derive(Default)]
pub struct ToolReleases {
    http: Client,
    /// Held across a lookup, so callers waiting on it share its answer.
    found: AsyncMutex<HashMap<Tool, Found>>,
}

struct Found {
    version: Option<String>,
    at: Instant,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
}

impl ToolReleases {
    /// The newest release of `tool`, such as `2.103.3`; the last one found
    /// while the releases cannot be reached, and `None` before any was.
    pub(crate) async fn latest(&self, tool: Tool) -> Option<String> {
        let mut found = self.found.lock().await;
        if let Some(found) = found.get(&tool) {
            let wait = if found.version.is_some() {
                FRESH
            } else {
                RETRY
            };
            if found.at.elapsed() < wait {
                return found.version.clone();
            }
        }
        let version = match timeout(LOOKUP_TIMEOUT, self.lookup(tool)).await {
            Ok(Ok(version)) => Some(version),
            Ok(Err(e)) => {
                log::info!("newest release not found; tool={tool} error={e:#}");
                None
            }
            Err(_) => {
                log::info!("newest release lookup timed out; tool={tool}");
                None
            }
        };
        let version = version.or_else(|| found.get(&tool)?.version.clone());
        found.insert(
            tool,
            Found {
                version: version.clone(),
                at: Instant::now(),
            },
        );
        version
    }

    async fn lookup(&self, tool: Tool) -> Result<String> {
        let repo = match tool.releases() {
            Releases::GitHub(repo) => repo,
            Releases::Channel(base) => {
                let text = self
                    .http
                    .get(format!("{base}/latest.txt"))
                    .header(USER_AGENT, "beep-server")
                    .send()
                    .await?
                    .error_for_status()?
                    .text()
                    .await?;
                return Ok(text.trim().to_owned());
            }
        };
        let url = format!("https://api.github.com/repos/{repo}/releases/latest");
        let release: Release = self
            .http
            .get(&url)
            .header(USER_AGENT, "beep-server")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let tag = release.tag_name;
        Ok(tag.strip_prefix('v').unwrap_or(&tag).to_owned())
    }
}

impl Online {
    /// Whether it installs newer tools over `tool-update/1`.
    pub(crate) fn installs_tools(&self) -> bool {
        self.offers(PROTOCOL)
    }

    /// Whether it installs newer releases of `tool`.
    pub(crate) fn installs(&self, tool: Tool) -> bool {
        self.installs_tools() && (tool != Tool::Flsh || self.offers(FLSH))
    }

    /// Has the node install release `version` of `tool`. `Err` holds the
    /// node's reason, in words for the user.
    pub(crate) async fn install_tool(&self, tool: Tool, version: &str) -> Result<()> {
        let (mut tx, mut rx) = self.open_stream(PROTOCOL).await?;
        let request = Request::Install {
            tool,
            version: version.to_owned(),
        };
        tx.send(&request).await?;
        let reply = timeout(INSTALL_TIMEOUT, rx.recv::<Reply>())
            .await
            .context("the install took too long")??
            .context("the node closed the install without an answer")?;
        match reply {
            Reply::Installed {} => Ok(()),
            Reply::Failed { reason } => bail!("{reason}"),
            other => bail!("the node answered {other:?}"),
        }
    }
}
