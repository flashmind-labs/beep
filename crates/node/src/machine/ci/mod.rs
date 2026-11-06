//! Watching the CI runs of the user's pinned projects, with no server
//! (#338). One machine per project watches: among the project's machines that
//! are online and have its checkout, the one that reaches the project's host
//! first, then the smallest endpoint id (`elect`). It lists the project's runs
//! every minute through `gh` or `teacli`, keeps the ones the signed-in user
//! started by a push or a pull request, waits on each going one with the
//! CLI's watch command, and tells the desktops once each ends (`book`), over
//! their `FollowRuns` streams. Entry point: `watch`, run by the node while it
//! is in a mesh.

mod book;
mod elect;
mod forge;
mod github;
mod projects;
mod watcher;

#[cfg(test)]
mod tests;

use crate::agents::AlertHub;
use beep_proto::node::{
    agent::RunNews,
    tools::{ForgeReach, HostReach},
};
use std::collections::BTreeMap;
use tokio::sync::{broadcast, watch};

pub(crate) use watcher::watch;

/// The most webhook deliveries waiting for the watcher.
const HITS: usize = 16;

/// What the watch shares with the rest of the node.
pub(crate) struct Ci {
    /// The runs told, for each desktop's `FollowRuns` stream.
    pub(crate) runs: AlertHub<RunNews>,
    /// How this node's calls to each host went, as `ForgeReach` answers.
    reach: watch::Sender<ForgeReach>,
    /// A repository (`owner/name`) whose webhook says a run changed.
    hits: broadcast::Sender<String>,
    /// The repositories whose webhooks this node takes now, with how many
    /// watches take each.
    live: watch::Sender<BTreeMap<String, usize>>,
}

impl Default for Ci {
    fn default() -> Self {
        Self {
            runs: AlertHub::default(),
            reach: watch::channel(ForgeReach::default()).0,
            hits: broadcast::channel(HITS).0,
            live: watch::channel(BTreeMap::new()).0,
        }
    }
}

impl Ci {
    /// How this node's calls to each host went.
    pub(crate) fn reach(&self) -> ForgeReach {
        self.reach.borrow().clone()
    }

    fn reach_of(&self, host: &str) -> HostReach {
        self.reach
            .borrow()
            .hosts
            .get(host)
            .copied()
            .unwrap_or_default()
    }

    fn record_reach(&self, host: &str, ok: bool) {
        self.reach.send_modify(|reach| {
            reach.hosts.entry(host.to_owned()).or_default().record(ok);
        });
    }

    /// A webhook delivery for `repo` says one of its runs changed.
    pub(crate) fn hit(&self, repo: &str) {
        // Nobody listening is fine: no watcher runs.
        let _ = self.hits.send(repo.to_owned());
    }

    /// Webhooks for `repo` come here until the guard is dropped.
    pub(crate) fn live(&self, repo: &str) -> Live<'_> {
        self.live
            .send_modify(|live| *live.entry(repo.to_owned()).or_default() += 1);
        Live {
            ci: self,
            repo: repo.to_owned(),
        }
    }

    fn is_live(&self, repo: &str) -> bool {
        self.live.borrow().contains_key(repo)
    }
}

/// Webhooks for one repository come to this node while it is held.
pub(crate) struct Live<'a> {
    ci: &'a Ci,
    repo: String,
}

impl Drop for Live<'_> {
    fn drop(&mut self) {
        self.ci.live.send_modify(|live| {
            if let Some(count) = live.get_mut(&self.repo) {
                *count -= 1;
                if *count == 0 {
                    live.remove(&self.repo);
                }
            }
        });
    }
}

/// Whether a webhook event says a run changed.
pub(crate) fn is_run_event(event: &str) -> bool {
    event == "workflow_run" || event.starts_with("action_run_")
}
