//! The children a session started on other machines, as its own node keeps
//! them: noted before the order goes, stopped with the parent (and again
//! until the machine says so), and kept over a restart.

use super::links::{Links, RemoteKid};
use crate::agents::Session;
use crate::agents::session::parent_gone;
use beep_proto::SessionId;
use iroh::EndpointId;
use std::sync::Mutex;
use tokio_util::task::AbortOnDropHandle;

impl Links {
    /// Notes a child that runs on another machine, and has it stopped with
    /// its parent.
    pub(crate) fn adopt_remote(&self, parent: &Session, kid: RemoteKid) {
        {
            let mut remote = crate::lock(&self.remote);

            if !kid.pending() && remote.contains(&kid) {
                return;
            }

            remote.push(kid.clone());
        }
        crate::events::open_changed();
        self.watch_kids(parent, kid.parent);
    }

    /// The child `kid` was told to start has started as `id`.
    pub(crate) fn settle_remote(&self, parent: &SessionId, machine: EndpointId, id: &SessionId) {
        {
            let mut remote = crate::lock(&self.remote);
            let known = remote
                .iter()
                .any(|kid| kid.parent == *parent && kid.machine == machine && kid.id == *id);
            let waiting = remote
                .iter()
                .position(|kid| kid.parent == *parent && kid.machine == machine && kid.pending());
            if let Some(at) = waiting {
                if known {
                    remote.remove(at);
                } else {
                    remote[at].id = id.to_owned();
                }
            }
        }
        crate::events::open_changed();
    }

    /// The machine turned a start down: nothing runs there for it.
    pub(crate) fn forget_expected(&self, parent: &SessionId, machine: EndpointId) {
        {
            let mut remote = crate::lock(&self.remote);

            if let Some(at) = remote
                .iter()
                .position(|kid| kid.parent == *parent && kid.machine == machine && kid.pending())
            {
                remote.remove(at);
            }
        }
        crate::events::open_changed();
    }

    /// The children of `parent` that run on other machines.
    pub(crate) fn remote_kids(&self, parent: &SessionId) -> Vec<RemoteKid> {
        crate::lock(&self.remote)
            .iter()
            .filter(|kid| kid.parent == *parent)
            .cloned()
            .collect()
    }

    /// Notes that a child ended or was stopped: it stays in the parent's
    /// list to read, but frees its place.
    pub(crate) fn end_remote(&self, kid: &RemoteKid) {
        mark_ended(&self.remote, kid);
    }

    /// Watches `parent`, which started children on other machines under the
    /// id `name`: when it ends for good they are stopped, again and again
    /// for a while if their machine does not answer.
    fn watch_kids(&self, parent: &Session, name: SessionId) {
        let mut live = parent.live.clone();
        let remote = self.remote.clone();
        let membership = self.membership.clone();
        let parent = name.clone();
        let task = tokio::spawn(async move {
            if !parent_gone(&mut live).await {
                return;
            }

            let Some(mesh) = membership.get() else {
                return;
            };
            let kids: Vec<RemoteKid> = crate::lock(&remote)
                .iter()
                .filter(|kid| kid.parent == parent && !kid.ended && !kid.pending())
                .cloned()
                .collect();
            let stops = kids.iter().map(|kid| async {
                match super::peer::stop_remote_retrying(&mesh, &parent, kid).await {
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
        crate::lock(&self.watchers).insert(name, AbortOnDropHandle::new(task));
    }
}

/// `kid` ended: it frees its place.
fn mark_ended(remote: &Mutex<Vec<RemoteKid>>, kid: &RemoteKid) {
    for known in crate::lock(remote).iter_mut() {
        if known.parent == kid.parent && known.machine == kid.machine && known.id == kid.id {
            known.ended = true;
        }
    }

    crate::events::open_changed();
}
