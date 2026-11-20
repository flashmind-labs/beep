//! The election of one machine among those that share a project, the same
//! on every node: the CI watcher and the team gateway use it.

use crate::agents::mcp::over_the_mesh;
use anyhow::{Result, anyhow, bail};
use beep_mesh::Mesh;
use beep_proto::{
    SessionId,
    node::tools::{ForgeReach, HostReach, Reply, Request, Tool},
};
use futures::future::join_all;
use iroh::EndpointId;
use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    time::Duration,
};
use tokio::time::timeout;

/// How long another machine has to say how it reaches the hosts.
const ASK_LIMIT: Duration = Duration::from_secs(10);

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

/// The other machines of `mesh` that run code and are online now.
pub(crate) fn online(mesh: &Mesh) -> BTreeSet<EndpointId> {
    mesh.code_hosts()
        .into_iter()
        .filter(|status| status.online && status.device.id != mesh.me())
        .map(|status| status.device.id)
        .collect()
}

/// What each of `peers` says of its reach, asked all at once with `ask`;
/// one that fails or does not answer in time is left out. `what` names the
/// asker in the log.
pub(crate) async fn gather<F, A>(
    what: &str,
    peers: impl IntoIterator<Item = EndpointId>,
    ask: F,
) -> BTreeMap<EndpointId, ForgeReach>
where
    F: Fn(EndpointId) -> A,
    A: Future<Output = Result<ForgeReach>>,
{
    let answers = join_all(peers.into_iter().map(|peer| {
        let asked = ask(peer);
        async move {
            let said = match timeout(ASK_LIMIT, asked).await {
                Ok(said) => said,
                Err(_) => Err(anyhow!("it did not answer in time")),
            };
            (peer, said)
        }
    }))
    .await;

    answers
        .into_iter()
        .filter_map(|(peer, said)| match said {
            Ok(reach) => Some((peer, reach)),
            Err(e) => {
                log::debug!("{what}: {peer} says nothing of its reach: {e:#}");
                None
            }
        })
        .collect()
}

/// How `peer` reaches the hosts; an error when it cannot say, as a node from
/// before the CI watch.
pub(crate) async fn ask_reach(mesh: &Mesh, peer: EndpointId) -> Result<ForgeReach> {
    let request = Request {
        session: SessionId::default(),
        tool: Tool::ForgeReach,
        args: "{}".to_owned(),
        approved: Default::default(),
    };

    match over_the_mesh(mesh, peer, &request).await? {
        Reply::Done {
            text,
            failed: false,
        } => Ok(serde_json::from_str(&text)?),
        Reply::Done { text, .. } => bail!("{text}"),
        _ => bail!("it answered something else"),
    }
}

#[cfg(test)]
#[path = "../tests/machine_elect.rs"]
mod tests;
