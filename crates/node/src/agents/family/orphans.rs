//! A child whose parent is on another machine follows that machine's
//! `agents` list. A parent that is gone from it, or a machine that drops and
//! stays away, for `ORPHAN_AFTER` closes the child, so a parent that never
//! got to stop it leaves no agent running for good. A machine from before
//! 0.8.28 does not serve its list to a node: it is asked every
//! `ORPHAN_CHECK` instead.

use super::links::Parent;
use crate::agents::session::{NODE_STOPPED, ended_for_good};
use crate::agents::{Session, family::Family, live::Live};
use crate::machine::elect::online;
use crate::membership::Membership;
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
    sync::Arc,
    time::Duration,
};
use tokio::{
    select,
    sync::{Notify, mpsc, watch},
    time::{Instant, sleep, sleep_until},
};
use tokio_util::task::AbortOnDropHandle;

/// How often a child asks a machine from before 0.8.28 (compat).
pub(super) const ORPHAN_CHECK: Duration = Duration::from_secs(60);
/// How long a follow waits before it dials a machine that dropped again.
const REDIAL: Duration = Duration::from_secs(5);
/// How long an ask that got no answer waits before it is asked again.
const REASK: Duration = Duration::from_secs(5);
/// How long the parent may be gone or silent before the child closes: long
/// enough for a restart of its node, which resumes it.
pub(super) const ORPHAN_AFTER: Duration = Duration::from_secs(600);

/// The parent's row in a pushed list, when it is there and not ended for
/// good (an end with the node resumes).
fn parent_row<'a>(sessions: &'a [SessionInfo], link: &ParentLink) -> Option<&'a SessionInfo> {
    sessions.iter().find(|info| {
        (info.id == link.session || info.id == link.key)
            && !(info.state.status == Status::Ended
                && info.state.error.as_deref() != Some(NODE_STOPPED))
    })
}

/// What a pushed list says of the parent.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Word {
    /// It is not in the list, or ended for good.
    Gone,
    /// Its row changed, or first showed. No field of it says whether it still
    /// keeps the child, so it is asked, paced by `asks`.
    Moved,
    /// Its row is as it was.
    Same,
}

/// What a pushed list says of the parent, given the row `last` seen.
pub(super) fn word(
    sessions: &[SessionInfo],
    last: &mut Option<SessionInfo>,
    link: &ParentLink,
) -> Word {
    let Some(row) = parent_row(sessions, link) else {
        *last = None;
        return Word::Gone;
    };

    if last.as_ref() == Some(row) {
        return Word::Same;
    }

    *last = Some(row.clone());

    Word::Moved
}

/// Asks whether the parent still keeps the child once `moved` rang, no
/// sooner than `ORPHAN_CHECK` after the last ask (kept across redials: this
/// runs for as long as the child is followed), so a burst of row changes is
/// one ask. An ask that gets no answer is a failed call: it is asked again
/// after `REASK` until it is answered or `there` says the parent is gone.
/// Ends when `ask` has no one to ask or `tx` is closed.
pub(super) async fn asks<F: Future<Output = Option<Parent>>>(
    moved: &Notify,
    mut there: watch::Receiver<bool>,
    mut ask: impl FnMut() -> F,
    tx: &mpsc::Sender<Parent>,
) {
    let mut last: Option<Instant> = None;

    loop {
        moved.notified().await;

        if let Some(at) = last {
            sleep_until(at + ORPHAN_CHECK).await;
        }

        loop {
            last = Some(Instant::now());

            let Some(answer) = ask().await else {
                return;
            };
            let unanswered = answer == Parent::Silent;

            if tx.send(answer).await.is_err() {
                return;
            }

            if !unanswered {
                break;
            }

            select! {
                () = sleep(REASK) => {}
                gone = watched::wait_for(&mut there, |there| !*there) => if gone.is_err() { return },
            }

            if !watched::read(&there, |there| *there) {
                break;
            }
        }
    }
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

impl Family {
    /// Has `session`, a child, tell its parent how it stands, and when the
    /// parent is on another machine check on it, for as long as it runs.
    pub(in crate::agents) fn watch_parent(&self, session: &Arc<Session>) {
        self.tell_parent(session);
        let Some(link) = session.info().parent else {
            return;
        };

        if link.background {
            self.delete.tidy_when_closed(session);
        }

        let Some(peer) = link.machine else {
            return;
        };
        let (membership, session_ref) = (self.links.membership.clone(), Arc::downgrade(session));
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
        let Some(parent_session) = self.sessions.get(parent) else {
            return false;
        };

        if watched::read(&parent_session.live, ended_for_good) {
            return false;
        }

        self.links.keeps(parent, machine, id, key)
    }
}

/// The child's side: follows `peer`'s word on the parent until the child
/// ends, and closes it when it is orphaned.
async fn check_parent(
    membership: Membership,
    session: std::sync::Weak<Session>,
    mut live: tokio::sync::watch::Receiver<Live>,
    peer: EndpointId,
    link: ParentLink,
) {
    let (tx, heard) = mpsc::channel(8);
    let parent = link.session.clone();
    let _feed = AbortOnDropHandle::new(tokio::spawn(feed(
        membership,
        session.clone(),
        peer,
        link,
        tx,
    )));

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
pub(super) async fn feed(
    membership: Membership,
    session: std::sync::Weak<Session>,
    peer: EndpointId,
    link: ParentLink,
    tx: mpsc::Sender<Parent>,
) {
    let mut meshes = membership.subscribe();
    let (moved, there) = (Arc::new(Notify::new()), watch::channel(true).0);
    // Asks for as long as the child is followed, so the pacing of its asks
    // holds over redials.
    let asker = {
        let (moved, there, tx, session) = (
            moved.clone(),
            there.subscribe(),
            tx.clone(),
            session.clone(),
        );
        let (meshes, link) = (meshes.clone(), link.clone());

        AbortOnDropHandle::new(tokio::spawn(async move {
            let ask = || async {
                let session = session.upgrade()?;
                let Some(mesh) = watched::read(&meshes, Clone::clone) else {
                    return Some(Parent::Silent);
                };
                let name = session.link_name();

                Some(super::peer::ask_parent(&mesh, peer, &link.session, &name, &session.id).await)
            };

            asks(&moved, there, ask, &tx).await;
        }))
    };

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

        match follow(
            &mesh,
            peer,
            &link,
            &tx,
            Rings {
                moved: &moved,
                there: &there,
                asker: &asker,
            },
        )
        .await
        {
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

/// What `follow` rings for the asker `feed` keeps.
pub(super) struct Rings<'a> {
    pub(super) moved: &'a Notify,
    /// Whether the parent is in the list, for an ask that is retried.
    pub(super) there: &'a watch::Sender<bool>,
    pub(super) asker: &'a AbortOnDropHandle<()>,
}

/// Passes on what each list `peer` pushes says of the parent, until the
/// stream ends: ringing the asker when the parent's row changes. `Ok` when
/// the child's side is gone.
async fn follow(
    mesh: &Mesh,
    peer: EndpointId,
    link: &ParentLink,
    tx: &mpsc::Sender<Parent>,
    rings: Rings<'_>,
) -> Result<()> {
    let mut watch = PeerWatch::open(mesh, peer, Topic::new("agents")).await?;
    let mut last = None;

    loop {
        let value = watch.next().await?;

        if hear_list(value, &mut last, link, tx, &rings).await {
            return Ok(());
        }
    }
}

/// What one pushed value says of the parent, passed on: ringing the asker
/// when its row changed. True when the child's side is gone.
pub(super) async fn hear_list(
    value: Value,
    last: &mut Option<SessionInfo>,
    link: &ParentLink,
    tx: &mpsc::Sender<Parent>,
    rings: &Rings<'_>,
) -> bool {
    let Value::Sessions(reply) = value else {
        return false;
    };
    let SessionsReply::Sessions { sessions } = *reply else {
        return false;
    };

    match word(&sessions, last, link) {
        Word::Gone => {
            rings.there.send_replace(false);

            if tx.send(Parent::Gone).await.is_err() {
                return true;
            }
        }
        Word::Moved => {
            rings.there.send_replace(true);
            rings.moved.notify_one();
        }
        Word::Same => {}
    }

    rings.asker.is_finished()
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
            super::peer::ask_parent(mesh, peer, &link.session, &session.link_name(), &session.id)
                .await;

        if tx.send(answer).await.is_err() {
            return;
        }
    }
}
