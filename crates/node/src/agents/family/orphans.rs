//! A child whose parent is on another machine asks that machine now and
//! then whether its parent still keeps it. A parent that is gone, or a
//! machine that stays silent, for `ORPHAN_AFTER` closes the child, so a
//! parent that never got to stop it leaves no agent running for good.

use super::{Agents, Session, children, ended_for_good};
use crate::link::Membership;
use iroh::EndpointId;
use std::{
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::time::{Instant, sleep};

/// How often a child asks.
const ORPHAN_CHECK: Duration = Duration::from_secs(60);
/// How long the parent may be gone or silent before the child closes: long
/// enough for a restart of its node, which resumes it.
pub(super) const ORPHAN_AFTER: Duration = Duration::from_secs(600);

/// What a child learned of its parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Parent {
    /// It keeps the child, or answered something that does not say it does
    /// not (a node too old to know the question).
    Keeps,
    /// Its node answered: the parent is gone or does not keep the child.
    Gone,
    /// Its node did not answer.
    Silent,
}

/// Whether the child is orphaned: the parent has not kept it since `silent`
/// began, for `ORPHAN_AFTER`.
pub(super) fn orphaned(answer: Parent, silent: &mut Option<Instant>, now: Instant) -> bool {
    match answer {
        Parent::Keeps => {
            *silent = None;
            false
        }
        Parent::Gone | Parent::Silent => {
            now.duration_since(*silent.get_or_insert(now)) >= ORPHAN_AFTER
        }
    }
}

impl Agents {
    /// Has `session`, started by a session on another machine, check on its
    /// parent for as long as it runs.
    pub(in crate::agents) fn watch_parent(&self, session: &Arc<Session>) {
        let Some(link) = session.info().parent else {
            return;
        };
        let Some(peer) = link.machine else {
            return;
        };
        let (membership, session_ref) = (self.family.membership.clone(), Arc::downgrade(session));
        let task = tokio::spawn(check_parent(membership, session_ref, peer, link.session));
        session.keep(task);
    }

    /// Whether this node's session `parent` keeps the child `id` (or `key`)
    /// that `machine` runs.
    pub(crate) fn keeps_child(
        &self,
        machine: EndpointId,
        parent: &str,
        id: &str,
        key: &str,
    ) -> bool {
        let Some(parent_session) = self.get(parent) else {
            return false;
        };
        if ended_for_good(&parent_session.live.borrow()) {
            return false;
        }
        self.family.keeps(parent, machine, id, key)
    }
}

/// The child's side: asks `peer` about `parent` until the child ends, and
/// closes it when it is orphaned.
async fn check_parent(
    membership: Arc<std::sync::OnceLock<Membership>>,
    session: Weak<Session>,
    peer: EndpointId,
    parent: String,
) {
    let mut silent = None;
    loop {
        sleep(ORPHAN_CHECK).await;
        let Some(session) = session.upgrade() else {
            return;
        };
        if session.live.borrow().status() == beep_proto::node::agent::Status::Ended {
            return;
        }
        let mesh = membership.get().and_then(Membership::get);
        let answer = match &mesh {
            Some(mesh) if mesh.me() == peer => return,
            Some(mesh) => {
                children::ask_parent(mesh, peer, &parent, &Agents::name_of(&session), &session.id)
                    .await
            }
            None => Parent::Silent,
        };
        if orphaned(answer, &mut silent, Instant::now()) {
            log::info!(
                "closing session {}: its parent {parent} is gone or unreachable",
                session.id
            );
            session.close().await;
            return;
        }
    }
}
