//! The election of one machine among those that share a project, the same
//! on every node: the CI watcher and the team gateway use it.

use beep_proto::node::tools::{ForgeReach, HostReach};
use iroh::EndpointId;
use std::collections::{BTreeMap, BTreeSet};

/// The machine that acts for a project, as this node sees it: `reach` is how
/// this machine reaches `host`, `peers` what the online machines that
/// answered said. Every node works it out from the same facts, so all pick
/// the same one: the candidate (an online machine of `machines` that
/// answered how it reaches the host) with the best `HostReach::rank`, then
/// the smallest id. Machines that keep failing still win when no other is
/// left, so a host that is down for everyone is tried again.
pub(crate) fn elect(
    machines: &BTreeSet<EndpointId>,
    me: EndpointId,
    reach: HostReach,
    peers: &BTreeMap<EndpointId, ForgeReach>,
    host: &str,
) -> Option<EndpointId> {
    let candidates = machines.iter().filter_map(|machine| {
        if *machine == me {
            return Some((me, reach));
        }

        let said = peers.get(machine)?;
        Some((*machine, said.hosts.get(host).copied().unwrap_or_default()))
    });
    candidates
        .min_by_key(|(id, reach)| (reach.rank(), *id))
        .map(|(id, _)| id)
}

#[cfg(test)]
#[path = "../tests/machine_elect.rs"]
mod tests;
