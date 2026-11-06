//! Which machine watches a project: every node works it out from the same
//! facts, so all pick the same one.

use beep_proto::node::tools::HostReach;
use iroh::EndpointId;

/// The machine that watches, from the candidates (the project's machines
/// that are online, have its checkout and answered how they reach its
/// host): the best `HostReach::rank`, then the smallest id. Machines that
/// keep failing still watch when no other is left, so a host that is down
/// for everyone is tried again.
pub(super) fn elect(
    candidates: impl IntoIterator<Item = (EndpointId, HostReach)>,
) -> Option<EndpointId> {
    candidates
        .into_iter()
        .min_by_key(|(id, reach)| (reach.rank(), *id))
        .map(|(id, _)| id)
}
