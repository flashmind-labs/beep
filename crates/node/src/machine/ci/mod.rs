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
use tokio::sync::watch;

pub(crate) use watcher::watch;

/// What the watch shares with the rest of the node.
pub(crate) struct Ci {
    /// The runs told, for each desktop's `FollowRuns` stream.
    pub(crate) runs: AlertHub<RunNews>,
    /// How this node's calls to each host went, as `ForgeReach` answers.
    reach: watch::Sender<ForgeReach>,
}

impl Default for Ci {
    fn default() -> Self {
        Self {
            runs: AlertHub::default(),
            reach: watch::channel(ForgeReach::default()).0,
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
}
