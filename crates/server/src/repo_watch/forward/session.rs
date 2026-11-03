//! One node forwarding one repository's webhook: the `webhook/1` session,
//! from the watch the server asks for to the node's last reply. Each
//! delivery reads the project's copies again; dropping the session has the
//! node stop the forward and delete its webhook.

use super::{LiveState, Refresh, Slot};
use crate::{
    app::App,
    credentials::WebhookOwner,
    nodes::Online,
    projects::model::{IssueHost, ProjectId, RepoName},
    protocol::{
        SecretText,
        node::webhook::{PollReason, Watch, WatchHost, WatchReply},
    },
    store::NewWebhook,
    user::User,
};
use anyhow::{Result, bail};
use chacha20poly1305::aead::{OsRng, rand_core::RngCore};
use secrecy::{ExposeSecret, SecretString};
use std::{
    fmt::Write,
    sync::{Arc, Weak},
};
use uuid::Uuid;

/// Replies in a row that do not read before the session is dropped.
const MAX_BAD: u32 = 3;

/// How a session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Ended {
    /// The node gives no live updates, saying why.
    Refused(PollReason),
    /// The stream closed: the node went offline, or its forward stopped.
    Closed,
}

/// What one session is for.
pub(super) struct Session<'a> {
    pub(super) app: Weak<App>,
    pub(super) user: &'a Arc<User>,
    pub(super) project: ProjectId,
    pub(super) host: &'a IssueHost,
    pub(super) repo: &'a RepoName,
    pub(super) slot: &'a Slot,
}

/// A secret for one webhook: 32 random bytes as hex.
fn new_secret() -> SecretString {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let mut hex = String::with_capacity(64);
    for byte in bytes {
        // Writing to a String cannot fail.
        let _ = write!(hex, "{byte:02x}");
    }
    SecretString::from(hex)
}

/// What the node is asked to watch.
fn watch_host(host: &IssueHost, repo: &RepoName) -> WatchHost {
    match host {
        IssueHost::Github => WatchHost::GitHub {
            token_var: crate::credentials::github_owner_var(repo.owner()),
        },
        IssueHost::Forgejo(host) => WatchHost::Forgejo {
            host: host.as_str().to_owned(),
        },
    }
}

impl Session<'_> {
    /// Pokes the project's copies, while the app lasts.
    fn poke(&self, refresh: Refresh) {
        if let Some(app) = self.app.upgrade() {
            app.repo_watch.poke(self.user.id(), self.project, refresh);
        }
    }

    /// Runs the session on `online` until it ends.
    pub(super) async fn run(&self, online: &Online) -> Result<Ended> {
        let id = Uuid::new_v4();
        let secret = new_secret();
        let sealed = {
            let Some(app) = self.app.upgrade() else {
                bail!("the server is stopping");
            };
            let owner = WebhookOwner {
                user: self.user.id(),
                id: &id.to_string(),
            };
            app.credential_key.seal_webhook(&owner, &secret)?
        };
        let stored = self.host.stored();
        let repo = self.repo.to_string();
        let stale = self
            .user
            .begin_webhook(&NewWebhook {
                id,
                host: &stored,
                repo: &repo,
                endpoint: &online.endpoint().to_string(),
                secret: &sealed,
            })
            .await?;
        let watch = Watch {
            host: watch_host(self.host, self.repo),
            repo: repo.clone(),
            secret: SecretText::new(secret.expose_secret()),
            stale,
        };
        // Held to the end: closing it ends the node's watch.
        let (_tx, mut rx) = online.watch_webhook(&watch).await?;
        let mut bad = 0;
        loop {
            let reply = match rx.recv::<WatchReply>().await {
                Ok(Some(reply)) => reply,
                Ok(None) => return Ok(Ended::Closed),
                Err(e) => {
                    bad += 1;
                    if bad >= MAX_BAD {
                        return Err(e);
                    }
                    log::debug!("webhooks: skipped a reply: {e:#}");
                    continue;
                }
            };
            bad = 0;
            match reply {
                WatchReply::Live { hook } => {
                    if let Some(hook) = hook {
                        self.user.webhook_made(id, hook).await?;
                    }
                    self.slot.set(LiveState::live());
                    // Whatever changed while it was polling.
                    self.poke(Refresh::BOTH);
                }
                WatchReply::Polling { why, detail } => {
                    log::info!("webhooks: a node gives no live updates ({why}): {detail}");
                    return Ok(Ended::Refused(why));
                }
                WatchReply::Event { event } => self.poke(Refresh::of_event(&event)),
                WatchReply::Removed { hook } => {
                    if let Err(e) = self.user.webhook_removed(&stored, &repo, hook).await {
                        log::warn!("webhooks: a removed webhook was not noted: {e:#}");
                    }
                }
                other => log::debug!("webhooks: skipped {other:?}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_webhook_gets_its_own_secret() {
        let (a, b) = (new_secret(), new_secret());
        assert_eq!(a.expose_secret().len(), 64);
        assert_ne!(a.expose_secret(), b.expose_secret());
    }
}
