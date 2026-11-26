//! The election of one machine among those that share a project, the same
//! on every node: the CI watcher and the team gateway use it.

use crate::{
    peer_stream::over_the_mesh,
    peer_stream::{PeerWatch, Refused},
};
use anyhow::{Result, anyhow, bail};
use beep_mesh::Mesh;
use beep_proto::{
    SessionId,
    node::{
        events::{FORGE_REACH_TOPIC, Topic, Value},
        tools::{ForgeReach, HostReach, Reply, Request, Tool},
    },
};
use futures::future::join_all;
use iroh::EndpointId;
use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{sync::watch, time::{sleep, timeout}};
use tokio_util::task::AbortOnDropHandle;

/// How long a follow waits before it follows a machine that dropped again.
const REDIAL: Duration = Duration::from_secs(5);

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
    ranked(machines, me, reach, peers, host).into_iter().next()
}

/// The candidates `elect` picks from, the one it picks first.
pub(crate) fn ranked(
    machines: &BTreeSet<EndpointId>,
    me: EndpointId,
    reach: HostReach,
    peers: &BTreeMap<EndpointId, ForgeReach>,
    host: &str,
) -> Vec<EndpointId> {
    let mut candidates: Vec<(EndpointId, HostReach)> = machines
        .iter()
        .filter_map(|machine| {
            if *machine == me {
                return Some((me, reach));
            }

            let said = peers.get(machine)?;
            Some((*machine, said.hosts.get(host).copied().unwrap_or_default()))
        })
        .collect();
    candidates.sort_by_key(|(id, reach)| (reach.rank(), *id));

    candidates.into_iter().map(|(id, _)| id).collect()
}

/// The other machines of `mesh` that run code and are online now.
pub(crate) fn online(mesh: &Mesh) -> BTreeSet<EndpointId> {
    mesh.code_hosts()
        .into_iter()
        .filter(|status| status.online && status.device.id != mesh.me())
        .map(|status| status.device.id)
        .collect()
}

/// What each of `asks` says of its reach, all awaited at once; one that
/// fails or does not answer in time is left out. `what` names the asker in
/// the log.
pub(crate) async fn gather<A>(
    what: &str,
    asks: impl IntoIterator<Item = (EndpointId, A)>,
) -> BTreeMap<EndpointId, ForgeReach>
where
    A: Future<Output = Result<ForgeReach>>,
{
    let answers = join_all(asks.into_iter().map(|(peer, asked)| async move {
        let said = match timeout(ASK_LIMIT, asked).await {
            Ok(said) => said,
            Err(_) => Err(anyhow!("it did not answer in time")),
        };
        (peer, said)
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

/// How the other machines reach the hosts, as each pushes it over
/// `forge-reach`. A machine that does not push, as one from before it, is
/// asked instead. Cheap to clone: the CI watcher and the team gateway share one.
///
/// The follow tasks hold `Hear`, never `Reaches`: they own the follow handles,
/// so dropping the last `Reaches` aborts them and lets go of the mesh.
#[derive(Clone)]
pub(crate) struct Reaches {
    hear: Hear,
    follows: Arc<Mutex<BTreeMap<EndpointId, AbortOnDropHandle<()>>>>,
}

/// What a follow task needs of `Reaches`.
#[derive(Clone)]
struct Hear {
    mesh: Mesh,
    said: Arc<Mutex<BTreeMap<EndpointId, Said>>>,
    /// Rings when a machine pushed a reach that differs from the last.
    changed: watch::Sender<()>,
}

enum Said {
    Reach(ForgeReach),
    /// The machine does not push: ask it.
    Refused,
}

impl Reaches {
    pub(crate) fn new(mesh: Mesh) -> Self {
        Self {
            hear: Hear {
                mesh,
                said: Arc::default(),
                changed: watch::Sender::new(()),
            },
            follows: Arc::default(),
        }
    }

    /// Rises when a machine pushes a reach that differs from the last.
    pub(crate) fn changes(&self) -> watch::Receiver<()> {
        self.hear.changed.subscribe()
    }

    /// How `peer` reaches the hosts: what it pushed last; asked once while it
    /// has not said, and each time while it does not push.
    pub(crate) fn ask(&self, peer: EndpointId) -> impl Future<Output = Result<ForgeReach>> + Send + 'static {
        let reaches = self.clone();

        async move {
            reaches.follow(peer);

            let pushed = match crate::lock(&reaches.hear.said).get(&peer) {
                Some(Said::Reach(reach)) => Some(reach.clone()),
                _ => None,
            };

            match pushed {
                Some(reach) => Ok(reach),
                None => ask_reach(&reaches.hear.mesh, peer).await,
            }
        }
    }

    /// Whether `peer` has pushed its reach and so keeps pushing changes.
    pub(crate) fn pushes(&self, peer: EndpointId) -> bool {
        matches!(crate::lock(&self.hear.said).get(&peer), Some(Said::Reach(_)))
    }

    /// Starts following `peer`, unless it is followed already.
    fn follow(&self, peer: EndpointId) {
        let mut follows = crate::lock(&self.follows);

        if follows.get(&peer).is_some_and(|follow| !follow.is_finished()) {
            return;
        }

        let hear = self.hear.clone();
        crate::lock(&hear.said).remove(&peer);
        follows.insert(
            peer,
            AbortOnDropHandle::new(tokio::spawn(async move { hear.hear(peer).await })),
        );
    }
}

impl Hear {
    /// Takes what `peer` pushes until it drops, then dials again while it is online.
    async fn hear(&self, peer: EndpointId) {
        loop {
            let ended = self.listen(peer).await;
            let refused = matches!(&ended, Err(e) if e.downcast_ref::<Refused>().is_some());
            {
                let mut said = crate::lock(&self.said);

                if refused {
                    said.insert(peer, Said::Refused);
                    return;
                }

                said.remove(&peer);
            }

            if let Err(e) = ended {
                log::debug!("the reach of {peer} is not followed: {e:#}");
            }

            sleep(REDIAL).await;

            if !online(&self.mesh).contains(&peer) {
                return;
            }
        }
    }

    async fn listen(&self, peer: EndpointId) -> Result<()> {
        let mut watch = PeerWatch::open(&self.mesh, peer, Topic::new(FORGE_REACH_TOPIC)).await?;

        loop {
            let Value::ForgeReach(reach) = watch.next().await? else {
                continue;
            };
            let changed = {
                let mut said = crate::lock(&self.said);
                let same = matches!(said.get(&peer), Some(Said::Reach(was)) if *was == reach);
                said.insert(peer, Said::Reach(reach));
                !same
            };

            if changed {
                self.changed.send_replace(());
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/machine_elect.rs"]
mod tests;
