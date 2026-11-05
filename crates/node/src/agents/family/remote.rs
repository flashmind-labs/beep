//! The children a session started on other machines, as its own node keeps
//! them: noted before the order goes, stopped with the parent (and again
//! until the machine says so), and kept over a restart.

use super::{Agents, Family, Session, children, parent_gone};
use crate::{agents::unix_now, link::Membership, link::restart};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{sync::Mutex, time::Duration};
use tokio_util::task::AbortOnDropHandle;

/// How long a child whose start got no answer counts, while its parent's node
/// cannot tell whether it runs.
pub(super) const PENDING_FOR: Duration = Duration::from_secs(120);

/// A child that runs on another machine, as its parent's node keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RemoteKid {
    /// The parent's id as its children's links name it.
    pub(crate) parent: String,
    pub(crate) machine: EndpointId,
    /// The machine as agents name it in `list_machines`.
    pub(crate) name: String,
    /// The child's id on that machine; empty while the order to start it has
    /// no answer yet, or got none.
    pub(crate) id: String,
    /// It ended or was stopped: it stays readable but no longer counts.
    #[serde(default)]
    pub(crate) ended: bool,
    /// When the node noted it, in Unix seconds.
    #[serde(default)]
    pub(crate) since: u64,
}

impl RemoteKid {
    /// A child the order to start is on its way for.
    pub(crate) fn expected(parent: &str, machine: EndpointId, name: &str) -> Self {
        Self {
            parent: parent.to_owned(),
            machine,
            name: name.to_owned(),
            id: String::new(),
            ended: false,
            since: unix_now(),
        }
    }

    pub(crate) fn pending(&self) -> bool {
        self.id.is_empty()
    }

    /// Whether it takes a place: it has not ended, and a start that got no
    /// answer is given the benefit of the doubt for a while only.
    pub(super) fn counts(&self) -> bool {
        !self.ended
            && (!self.pending() || unix_now().saturating_sub(self.since) < PENDING_FOR.as_secs())
    }
}

impl Agents {
    /// Notes a child that runs on another machine, and has it stopped with
    /// its parent.
    pub(in crate::agents) fn adopt_remote(&self, parent: &Session, kid: RemoteKid) {
        {
            let mut remote = crate::lock(&self.family.remote);
            if !kid.pending() && remote.contains(&kid) {
                return;
            }
            remote.push(kid.clone());
        }
        restart::changed();
        self.watch_kids(parent, kid.parent);
    }

    /// The child `kid` was told to start has started as `id`.
    pub(crate) fn settle_remote(&self, parent: &str, machine: EndpointId, id: &str) {
        {
            let mut remote = crate::lock(&self.family.remote);
            let known = remote
                .iter()
                .any(|kid| kid.parent == parent && kid.machine == machine && kid.id == id);
            let waiting = remote
                .iter()
                .position(|kid| kid.parent == parent && kid.machine == machine && kid.pending());
            if let Some(at) = waiting {
                if known {
                    remote.remove(at);
                } else {
                    remote[at].id = id.to_owned();
                }
            }
        }
        restart::changed();
    }

    /// The machine turned a start down: nothing runs there for it.
    pub(crate) fn forget_expected(&self, parent: &str, machine: EndpointId) {
        {
            let mut remote = crate::lock(&self.family.remote);
            if let Some(at) = remote
                .iter()
                .position(|kid| kid.parent == parent && kid.machine == machine && kid.pending())
            {
                remote.remove(at);
            }
        }
        restart::changed();
    }

    /// The children of `parent` that run on other machines.
    pub(crate) fn remote_kids(&self, parent: &str) -> Vec<RemoteKid> {
        crate::lock(&self.family.remote)
            .iter()
            .filter(|kid| kid.parent == parent)
            .cloned()
            .collect()
    }

    /// Notes that a child ended or was stopped: it stays in the parent's
    /// list to read, but frees its place.
    pub(crate) fn end_remote(&self, kid: &RemoteKid) {
        mark_ended(&self.family.remote, kid);
    }

    /// The links the open sessions keep over a restart: the remote children
    /// of the session `name`.
    pub(in crate::agents) fn kids_to_keep(&self, name: &str) -> Vec<RemoteKid> {
        let mut kids = self.remote_kids(name);
        kids.retain(|kid| !kid.pending());
        kids
    }

    /// Watches `parent`, which started children on other machines under the
    /// id `name`: when it ends for good they are stopped, again and again
    /// for a while if their machine does not answer.
    fn watch_kids(&self, parent: &Session, name: String) {
        let mut live = parent.live.clone();
        let remote = self.family.remote.clone();
        let membership = self.family.membership.clone();
        let parent = name.clone();
        let task = tokio::spawn(async move {
            if !parent_gone(&mut live).await {
                return;
            }
            let Some(mesh) = membership.get().and_then(Membership::get) else {
                return;
            };
            let kids: Vec<RemoteKid> = crate::lock(&remote)
                .iter()
                .filter(|kid| kid.parent == parent && !kid.ended && !kid.pending())
                .cloned()
                .collect();
            let stops = kids.iter().map(|kid| async {
                match children::stop_remote_retrying(&mesh, &parent, kid).await {
                    Ok(()) => mark_ended(&remote, kid),
                    Err(e) => log::warn!(
                        "child {} on {} was not stopped: {e}; it closes itself once it finds \
                         its parent gone",
                        kid.id,
                        kid.name
                    ),
                }
            });
            futures::future::join_all(stops).await;
        });
        crate::lock(&self.family.watchers).insert(name, AbortOnDropHandle::new(task));
    }
}

/// `kid` ended: it frees its place.
fn mark_ended(remote: &Mutex<Vec<RemoteKid>>, kid: &RemoteKid) {
    for known in crate::lock(remote).iter_mut() {
        if known.parent == kid.parent && known.machine == kid.machine && known.id == kid.id {
            known.ended = true;
        }
    }
    restart::changed();
}

impl Family {
    /// Whether `kid` is one `parent` keeps on `machine`: by the id its machine
    /// lists it under or the one it had when it started.
    pub(super) fn keeps(&self, parent: &str, machine: EndpointId, id: &str, key: &str) -> bool {
        crate::lock(&self.remote).iter().any(|kid| {
            kid.parent == parent
                && kid.machine == machine
                && kid.counts()
                && (kid.pending() || kid.id == id || kid.id == key)
        })
    }
}
