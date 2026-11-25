//! What the node's CI watch (`machine::ci`) shares with its agents and peers.

use super::alerts::AlertHub;
use beep_proto::node::{
    agent::RunNews,
    events::{FORGE_REACH_TOPIC, Topic},
    tools::{ForgeReach, HostReach},
};
use beep_proto::watched;
use tokio::sync::watch;

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
        watched::read_sent(&self.reach, Clone::clone)
    }

    /// Rises when how a call to a host went changes.
    pub(crate) fn reaches(&self) -> watch::Receiver<ForgeReach> {
        self.reach.subscribe()
    }

    pub(crate) fn reach_of(&self, host: &str) -> HostReach {
        watched::read_sent(&self.reach, |reach| {
            reach.hosts.get(host).copied().unwrap_or_default()
        })
    }

    pub(crate) fn record_reach(&self, host: &str, ok: bool) {
        // Only a call that changes how the host is reached wakes the gateway.
        let changed = self.reach.send_if_modified(|reach| {
            let host = reach.hosts.entry(host.to_owned()).or_default();
            let before = *host;
            host.record(ok);

            *host != before
        });

        if changed {
            crate::events::system().changed(Topic::new(FORGE_REACH_TOPIC));
        }
    }
}
