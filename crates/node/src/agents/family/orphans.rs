//! A child whose parent is on another machine follows that machine's
//! `agents` list. A parent that is gone from it, or a machine that drops and
//! stays away, for `ORPHAN_AFTER` closes the child, so a parent that never
//! got to stop it leaves no agent running for good. A machine from before
//! 0.8.28 does not serve its list to a node: it is asked every
//! `ORPHAN_CHECK` instead.

use super::{Agents, Session};
use crate::agents::session::{NODE_STOPPED, ended_for_good};
use crate::link::Membership;
use crate::machine::elect::online;
use crate::peer_stream::{PeerWatch, Refused};
use anyhow::Result;
use beep_mesh::Mesh;
use beep_proto::node::{
    agent::{ParentLink, SessionInfo, SessionsReply, Status},
    events::{Topic, Value},
};
use beep_proto::{SessionId, watched};
use iroh::EndpointId;
use std::{
    future::{Future, pending},
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::{
    select,
    sync::mpsc,
    time::{Instant, sleep, sleep_until},
};
use tokio_util::task::AbortOnDropHandle;

/// How often a child asks a machine from before 0.8.28 (compat).
const ORPHAN_CHECK: Duration = Duration::from_secs(60);
/// How long a follow waits before it dials a machine that dropped again.
const REDIAL: Duration = Duration::from_secs(5);
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

/// The parent's row in a pushed list, when it is there and not ended for
/// good (an end with the node resumes).
fn parent_row<'a>(sessions: &'a [SessionInfo], link: &ParentLink) -> Option<&'a SessionInfo> {
    sessions.iter().find(|info| {
        (info.id == link.session || info.id == link.key)
            && !(info.state.status == Status::Ended
                && info.state.error.as_deref() != Some(NODE_STOPPED))
    })
}

/// What a pushed list says of the parent, given the row `last` seen. A parent
/// that is gone from it is gone. One whose row changed (or first showed) is
/// asked whether it still keeps the child, which a list does not say. One
/// whose row did not change says nothing new: `None`.
pub(super) async fn word<F: Future<Output = Parent>>(
    sessions: &[SessionInfo],
    last: &mut Option<SessionInfo>,
    link: &ParentLink,
    ask: impl FnOnce() -> F,
) -> Option<Parent> {
    let Some(row) = parent_row(sessions, link) else {
        *last = None;
        return Some(Parent::Gone);
    };

    if last.as_ref() == Some(row) {
        return None;
    }

    *last = Some(row.clone());

    Some(ask().await)
}

/// Reads what `heard` says of the parent until the child is orphaned (true):
/// gone or silent since `ORPHAN_AFTER`, with no word that it keeps the child
/// in between. False once `heard` ends.
pub(super) async fn orphaned(mut heard: mpsc::Receiver<Parent>) -> bool {
    let mut silent: Option<Instant> = None;

    loop {
        let due = async {
            match silent {
                Some(since) => sleep_until(since + ORPHAN_AFTER).await,
                None => pending().await,
            }
        };

        select! {
            answer = heard.recv() => match answer {
                Some(Parent::Keeps) => silent = None,
                Some(Parent::Gone | Parent::Silent) => {
                    silent.get_or_insert_with(Instant::now);
                }
                None => return false,
            },
            () = due => return true,
        }
    }
}

impl Agents {
    /// Has `session`, a child, tell its parent how it stands, and when the
    /// parent is on another machine check on it, for as long as it runs.
    pub(in crate::agents) fn watch_parent(&self, session: &Arc<Session>) {
        self.tell_parent(session);
        let Some(link) = session.info().parent else {
            return;
        };

        if link.background {
            self.tidy_when_closed(session);
        }

        let Some(peer) = link.machine else {
            return;
        };
        let (membership, session_ref) = (self.family.membership.clone(), Arc::downgrade(session));
        let live = session.live.clone();
        let task = tokio::spawn(check_parent(membership, session_ref, live, peer, link));
        session.keep(task);
    }

    /// Whether this node's session `parent` keeps the child `id` (or `key`)
    /// that `machine` runs.
    pub(crate) fn keeps_child(
        &self,
        machine: EndpointId,
        parent: &SessionId,
        id: &SessionId,
        key: &SessionId,
    ) -> bool {
        let Some(parent_session) = self.get(parent) else {
            return false;
        };

        if watched::read(&parent_session.live, ended_for_good) {
            return false;
        }

        self.family.keeps(parent, machine, id, key)
    }
}

/// The child's side: follows `peer`'s word on the parent until the child
/// ends, and closes it when it is orphaned.
async fn check_parent(
    membership: Arc<OnceLock<Membership>>,
    session: std::sync::Weak<Session>,
    mut live: tokio::sync::watch::Receiver<super::Live>,
    peer: EndpointId,
    link: ParentLink,
) {
    let (tx, heard) = mpsc::channel(8);
    let parent = link.session.clone();
    let _feed = AbortOnDropHandle::new(tokio::spawn(feed(membership, session.clone(), peer, link, tx)));

    select! {
        orphan = orphaned(heard) => {
            let Some(session) = session.upgrade().filter(|_| orphan) else {
                return;
            };

            log::info!(
                "closing session {}: its parent {parent} is gone or unreachable",
                session.id
            );
            session.close().await;
        }
        _ = watched::wait_for(&mut live, |live| live.status() == Status::Ended) => {}
    }
}

/// Tells `tx` how the parent stands: from each list `peer` pushes, and
/// silent while the machine is away. Ends when the child's side does.
async fn feed(
    membership: Arc<OnceLock<Membership>>,
    session: std::sync::Weak<Session>,
    peer: EndpointId,
    link: ParentLink,
    tx: mpsc::Sender<Parent>,
) {
    let Some(membership) = membership.get().cloned() else {
        return;
    };
    let mut meshes = membership.subscribe();

    loop {
        let mesh = watched::read(&meshes, Clone::clone);

        let Some(mesh) = mesh else {
            // Not in a mesh: whatever the parent's machine does is unknown.
            if tx.send(Parent::Silent).await.is_err() || meshes.changed().await.is_err() {
                return;
            }

            continue;
        };

        if mesh.me() == peer {
            return;
        }

        let mut updates = mesh.updates();

        // Presence moves the follow: dial while the machine is online, wait
        // for the mesh to say it is back while it is not.
        if !online(&mesh).contains(&peer) {
            if tx.send(Parent::Silent).await.is_err() {
                return;
            }

            select! {
                changed = updates.changed() => if changed.is_err() { return },
                changed = meshes.changed() => if changed.is_err() { return },
            }

            continue;
        }

        match follow(&mesh, &session, peer, &link, &tx).await {
            Err(e) if e.downcast_ref::<Refused>().is_some() => {
                // Compat, until nodes before 0.8.28 are gone: it does not
                // serve its list to a node, so it is asked.
                ask_every(&mesh, &session, peer, &link, &tx).await;
                return;
            }
            Err(e) => log::debug!("the parent {} is not followed: {e:#}", link.session),
            Ok(()) => return,
        }

        if tx.send(Parent::Silent).await.is_err() {
            return;
        }

        // Retry while the machine stays online; a drop it shows is waited on above.
        sleep(REDIAL).await;
    }
}

/// Passes on what each list `peer` pushes says of the parent, until the
/// stream ends: asking `peer` once whenever the parent's row changes whether
/// it still keeps the child. `Ok` when the child's side is gone.
async fn follow(
    mesh: &Mesh,
    session: &std::sync::Weak<Session>,
    peer: EndpointId,
    link: &ParentLink,
    tx: &mpsc::Sender<Parent>,
) -> Result<()> {
    let mut watch = PeerWatch::open(mesh, peer, Topic::new("agents")).await?;
    let mut last = None;

    loop {
        let Value::Sessions(reply) = watch.next().await? else {
            continue;
        };
        let SessionsReply::Sessions { sessions } = *reply else {
            continue;
        };
        let Some(session) = session.upgrade() else {
            return Ok(());
        };
        let name = Agents::name_of(&session);
        let ask = || async {
            super::peer::ask_parent(mesh, peer, &link.session, &name, &session.id).await
        };

        if let Some(word) = word(&sessions, &mut last, link, ask).await
            && tx.send(word).await.is_err()
        {
            return Ok(());
        }
    }
}

/// Asks a machine that does not push its list, every `ORPHAN_CHECK`.
async fn ask_every(
    mesh: &Mesh,
    session: &std::sync::Weak<Session>,
    peer: EndpointId,
    link: &ParentLink,
    tx: &mpsc::Sender<Parent>,
) {
    loop {
        sleep(ORPHAN_CHECK).await;
        let Some(session) = session.upgrade() else {
            return;
        };
        let answer =
            super::peer::ask_parent(mesh, peer, &link.session, &Agents::name_of(&session), &session.id)
                .await;

        if tx.send(answer).await.is_err() {
            return;
        }
    }
}
