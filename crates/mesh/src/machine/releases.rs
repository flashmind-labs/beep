//! The newest release of each tool a node installs (`gh`, `teacli`,
//! `flsh`), looked up every few hours and shared by whoever asks.

use anyhow::Result;
use beep_proto::node::tool_update::{Releases, Tool};
use reqwest::{Client, header::USER_AGENT};
use serde::Deserialize;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio::{sync::Mutex as AsyncMutex, time::timeout};

/// What the releases' hosts are told this is.
const AGENT: &str = "beep";

/// How long a found release is trusted before it is looked up again.
const FRESH: Duration = Duration::from_secs(6 * 60 * 60);
/// How long a failed lookup waits before the next try.
const RETRY: Duration = Duration::from_secs(15 * 60);
/// How long one lookup may take.
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(10);

/// The newest release of each tool, as last looked up.
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

impl Default for ToolReleases {
    fn default() -> Self {
        let http = Client::builder()
            .timeout(LOOKUP_TIMEOUT)
            .build()
            .unwrap_or_default();
        Self {
            http,
            found: AsyncMutex::default(),
        }
    }
}

impl ToolReleases {
    /// The newest release of `tool`, such as `2.103.3`; the last one found
    /// while the releases cannot be reached, and `None` before any was.
    pub async fn latest(&self, tool: Tool) -> Option<String> {
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
                tracing::info!("newest release not found; tool={tool} error={e:#}");
                None
            }
            Err(_) => {
                tracing::info!("newest release lookup timed out; tool={tool}");
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
                    .header(USER_AGENT, AGENT)
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
            .header(USER_AGENT, AGENT)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let tag = release.tag_name;
        Ok(tag.strip_prefix('v').unwrap_or(&tag).to_owned())
    }
}
