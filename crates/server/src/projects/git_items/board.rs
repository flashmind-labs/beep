//! Each user's open items across their git hosts, kept for a few minutes.

use crate::{
    AsyncMutex,
    account::credentials::{host_client, is_public},
    protocol::{GitItems, LabeledGitItems},
    store::GitLogin,
};
use async_trait::async_trait;
use beep_projects::{
    git_items::board::{Login, read},
    imports::hosts::Hosts,
    model::ForgejoUrl,
};
use reqwest::Client;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;

/// How long a list is kept when every host answered.
const FRESH: Duration = Duration::from_secs(180);
/// How long it is kept when a host failed, to try that one again sooner.
const FRESH_AFTER_ERROR: Duration = Duration::from_secs(60);
/// Calls to git hosts in flight at once, server-wide.
const MAX_CALLS: usize = 16;

/// What one user was last given.
struct Cached {
    /// The credential ids it was read with, so a token change rereads.
    logins: Vec<String>,
    at: Instant,
    items: LabeledGitItems,
}

/// Each user's last list, locked per user so concurrent asks read the hosts once.
pub struct GitBoard {
    users: Mutex<HashMap<String, Arc<AsyncMutex<Option<Cached>>>>>,
    calls: Arc<Semaphore>,
}

impl Default for GitBoard {
    fn default() -> Self {
        Self::new()
    }
}

impl GitBoard {
    pub fn new() -> Self {
        Self {
            users: Mutex::default(),
            calls: Arc::new(Semaphore::new(MAX_CALLS)),
        }
    }

    /// The server-wide limit on git host calls in flight.
    pub(crate) fn calls(&self) -> &Arc<Semaphore> {
        &self.calls
    }

    /// `user`'s open items with `logins`, from the cache while it is fresh.
    pub async fn list(&self, user: &str, logins: Vec<GitLogin>) -> anyhow::Result<LabeledGitItems> {
        if logins.is_empty() {
            return Ok(LabeledGitItems {
                found: GitItems {
                    items: Vec::new(),
                    hosts: Vec::new(),
                },
                labels: Vec::new(),
            });
        }
        let slot = self
            .users
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(user.to_owned())
            .or_default()
            .clone();
        // Held across the reads on purpose: a second ask waits for the
        // first and takes its list.
        let mut cached = slot.lock().await;
        let ids: Vec<String> = logins.iter().map(|login| login.id.clone()).collect();
        if let Some(last) = cached.as_ref() {
            let ttl = if last
                .items
                .found
                .hosts
                .iter()
                .any(|host| host.error.is_some())
            {
                FRESH_AFTER_ERROR
            } else {
                FRESH
            };
            if last.logins == ids && last.at.elapsed() < ttl {
                return Ok(last.items.clone());
            }
        }
        let hosts = BoardHosts {
            calls: self.calls.clone(),
        };
        let reads: Vec<Login<'_>> = logins
            .iter()
            .map(|login| Login {
                slot: &login.slot,
                secret: &login.secret,
                account: login.account.as_deref(),
            })
            .collect();
        let items = read(&hosts, &reads).await?;
        *cached = Some(Cached {
            logins: ids,
            at: Instant::now(),
            items: items.clone(),
        });
        Ok(items)
    }
}

/// The server's calls to git hosts: one bounded client that reaches only public addresses.
struct BoardHosts {
    calls: Arc<Semaphore>,
}

#[async_trait]
impl Hosts for BoardHosts {
    fn client(&self) -> anyhow::Result<Client> {
        host_client()
    }

    fn calls(&self) -> &Arc<Semaphore> {
        &self.calls
    }

    async fn is_public(&self, url: &ForgejoUrl) -> bool {
        is_public(url).await
    }
}
