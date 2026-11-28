//! The election of one machine among those that share a project, the same
//! on every node: the CI watcher and the team gateway use it.

use crate::{
    peer_stream::over_the_mesh,
    peer_stream::{PeerWatch, REDIAL, Refused},
};
use anyhow::{Result, bail};
use beep_mesh::Mesh;
use beep_proto::{
    SessionId,
    node::{
        events::{FORGE_REACH_TOPIC, Topic, Value},
        tools::{ForgeReach, HostReach, Reply, Request, Tool},
    },
};
use iroh::EndpointId;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    select,
    sync::watch,
    time::{sleep, timeout},
};
use tokio_util::task::AbortOnDropHandle;

/// How often a machine from before 0.8.28, which does not push its reach, is
/// asked for it, and how often a quiet project's runs are listed.
pub(crate) const QUIET: Duration = Duration::from_secs(10 * 60);

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
/// `forge-reach`; a machine that does not push, as one from before 0.8.28, is
/// asked every `QUIET`. It is the one owner of what they said: the CI watcher
/// and the team gateway track the machines they need and elect on `view`.
/// Cheap to clone: they share one.
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
    /// Rises when what a machine said changes.
    changed: watch::Sender<()>,
    #[cfg(test)]
    script: Arc<Mutex<std::collections::VecDeque<Streamed>>>,
    /// A scripted machine stays online.
    #[cfg(test)]
    up: bool,
}

/// What a test makes the stream of a machine do, one per dial.
#[cfg(test)]
pub(crate) enum Streamed {
    Refuse,
    Fail,
    Push(ForgeReach),
}

#[derive(Clone, PartialEq)]
pub(crate) enum Said {
    /// Followed; nothing heard yet, and no answer to an ask yet.
    Pending,
    Reach(ForgeReach),
    /// Asked (an old node, or one whose follow failed) and no answer in time.
    Silent,
}

impl Reaches {
    pub(crate) fn new(mesh: Mesh) -> Self {
        Self {
            hear: Hear {
                mesh,
                said: Arc::default(),
                changed: watch::Sender::new(()),
                #[cfg(test)]
                script: Arc::default(),
                #[cfg(test)]
                up: false,
            },
            follows: Arc::default(),
        }
    }

    /// A `Reaches` whose streams do what `script` says, one per dial, and
    /// whose machines stay online.
    #[cfg(test)]
    pub(crate) fn scripted(mesh: Mesh, script: impl IntoIterator<Item = Streamed>) -> Self {
        let reaches = Self::new(mesh);
        crate::lock(&reaches.hear.script).extend(script);

        Self {
            hear: Hear {
                up: true,
                ..reaches.hear
            },
            follows: reaches.follows,
        }
    }

    /// Sets what `peer` said.
    #[cfg(test)]
    pub(crate) fn put_for_test(&self, peer: EndpointId, said: Said) {
        self.hear.put(peer, said);
    }

    /// Rises when what a machine said changes.
    pub(crate) fn changes(&self) -> watch::Receiver<()> {
        self.hear.changed.subscribe()
    }

    /// Follows every machine of `peers` not followed yet. Idempotent. A
    /// follow that ended, as when its machine went offline, starts again.
    pub(crate) fn track(&self, peers: impl IntoIterator<Item = EndpointId>) {
        let mut follows = crate::lock(&self.follows);

        for peer in peers {
            // A follow takes its entry away as its last act, under this lock,
            // so a missing entry means the follow is over or about to be.
            let running = {
                let mut said = crate::lock(&self.hear.said);
                let known = said.contains_key(&peer);
                said.entry(peer).or_insert(Said::Pending);

                known
                    && follows
                        .get(&peer)
                        .is_some_and(|follow| !follow.is_finished())
            };

            if running {
                continue;
            }

            let hear = self.hear.clone();
            follows.insert(
                peer,
                AbortOnDropHandle::new(tokio::spawn(async move { hear.hear(peer).await })),
            );
        }
    }

    /// Whether every machine of `wanted` has said something, a reach or
    /// silence after an ask: what an election waits for.
    pub(crate) fn settled(&self, wanted: &BTreeSet<EndpointId>) -> bool {
        let said = crate::lock(&self.hear.said);

        wanted
            .iter()
            .all(|peer| matches!(said.get(peer), Some(Said::Reach(_) | Said::Silent)))
    }

    /// What the machines said, for one election: a value, taken now.
    pub(crate) fn view(&self) -> BTreeMap<EndpointId, ForgeReach> {
        crate::lock(&self.hear.said)
            .iter()
            .filter_map(|(peer, said)| match said {
                Said::Reach(reach) => Some((*peer, reach.clone())),
                Said::Pending | Said::Silent => None,
            })
            .collect()
    }
}

impl Hear {
    /// Takes what `peer` pushes until it drops, then dials again while it is
    /// online. Until it pushes, it is asked once. A node before 0.8.28 refuses
    /// the stream: it is asked every `QUIET`, and the stream tried again then,
    /// for it may have upgraded.
    async fn hear(&self, peer: EndpointId) {
        let mut refused = false;

        loop {
            let mut pushed = false;
            let ended = {
                let listening = self.listen(peer, &mut pushed);
                let asking = self.ask_first(peer);
                tokio::pin!(listening, asking);
                let mut asked = false;

                loop {
                    select! {
                        ended = &mut listening => break ended,
                        () = &mut asking, if !asked => asked = true,
                    }
                }
            };
            let was_refused = refused;
            refused = matches!(&ended, Err(e) if e.downcast_ref::<Refused>().is_some());

            if !pushed && (self.is_pending(peer) || (refused && was_refused)) {
                // The ask that raced the stream was cut short, or this machine
                // still does not push: ask it, so that it never stays pending.
                let said = self.ask(peer).await;
                self.put(peer, said);
            }

            if pushed {
                self.forget(peer);
            }

            if let Err(e) = ended
                && !refused
            {
                log::debug!("the reach of {peer} is not followed: {e:#}");
            }

            if refused {
                sleep(QUIET).await;
            } else {
                sleep(REDIAL).await;
            }

            if !self.is_up(peer) {
                self.leave(peer);
                return;
            }
        }
    }

    /// Asks `peer` once when nothing was heard of it yet: a push that comes
    /// meanwhile wins, and an ask that fails or runs out of time says nothing.
    async fn ask_first(&self, peer: EndpointId) {
        if !self.is_pending(peer) {
            return;
        }

        let said = self.ask(peer).await;

        if self.is_pending(peer) {
            self.put(peer, said);
        }
    }

    fn is_up(&self, peer: EndpointId) -> bool {
        #[cfg(test)]
        if self.up {
            return true;
        }

        online(&self.mesh).contains(&peer)
    }

    /// Drops what `peer` said as its follow ends, and rings so that whoever
    /// needs it tracks it again.
    fn leave(&self, peer: EndpointId) {
        if crate::lock(&self.said).remove(&peer).is_some() {
            self.changed.send_replace(());
        }
    }

    /// What `peer` says of its reach within `ASK_LIMIT`.
    async fn ask(&self, peer: EndpointId) -> Said {
        match timeout(ASK_LIMIT, ask_reach(&self.mesh, peer)).await {
            Ok(Ok(reach)) => Said::Reach(reach),
            Ok(Err(e)) => {
                log::debug!("{peer} says nothing of its reach: {e:#}");
                Said::Silent
            }
            Err(_) => {
                log::debug!("{peer} did not say how it reaches the hosts in time");
                Said::Silent
            }
        }
    }

    fn is_pending(&self, peer: EndpointId) -> bool {
        matches!(
            crate::lock(&self.said).get(&peer),
            None | Some(Said::Pending)
        )
    }

    /// Keeps what `peer` said, and rings when that differs from the last.
    fn put(&self, peer: EndpointId, said: Said) {
        let changed = crate::lock(&self.said).insert(peer, said.clone()) != Some(said);

        if changed {
            self.changed.send_replace(());
        }
    }

    /// Takes back a reach `peer` pushed, and rings: it no longer pushes, so
    /// it is asked again.
    fn forget(&self, peer: EndpointId) {
        let pushed = matches!(crate::lock(&self.said).get(&peer), Some(Said::Reach(_)));

        if pushed {
            self.put(peer, Said::Pending);
        }
    }

    /// Takes what `peer` pushes until the stream ends; `pushed` says whether
    /// it delivered one.
    async fn listen(&self, peer: EndpointId, pushed: &mut bool) -> Result<()> {
        #[cfg(test)]
        let step = crate::lock(&self.script).pop_front();
        #[cfg(test)]
        if let Some(step) = step {
            return match step {
                Streamed::Refuse => Err(Refused("not before 0.8.28".to_owned()).into()),
                Streamed::Fail => bail!("the stream did not open"),
                Streamed::Push(reach) => {
                    *pushed = true;
                    self.put(peer, Said::Reach(reach));
                    std::future::pending().await
                }
            };
        }

        let mut watch = PeerWatch::open(&self.mesh, peer, Topic::new(FORGE_REACH_TOPIC)).await?;

        loop {
            let Value::ForgeReach(reach) = watch.next().await? else {
                continue;
            };
            *pushed = true;
            self.put(peer, Said::Reach(reach));
        }
    }
}

#[cfg(test)]
#[path = "../tests/machine_elect.rs"]
mod tests;
