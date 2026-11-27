//! What the node keeps of the family trees it takes part in, apart from the
//! code that works on them: the children on other machines, the waits, the
//! starts under way, and the types the family files pass to one another.

use crate::agents::{child_view::ChildView, live::Live};
use crate::{logs::unix_now, membership::Membership, tags};
use beep_proto::SessionId;
use beep_proto::{AccountName, CustomAgent, ModelId};
use beep_proto::node::agent::Effort;
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use tokio::{
    sync::Notify,
    time::Instant,
};
use tokio_util::task::AbortOnDropHandle;

/// How many starts deep a chain may go: a session started by a session
/// started by a session started by one the user began.
pub(crate) const MAX_DEPTH: u8 = 3;
/// The most live descendants one root session may have, and the most direct
/// children one session may have.
pub(crate) const MAX_CHILDREN: usize = 8;
/// The session a chain of starts began from, and its machine.
pub(super) type Root = (Option<EndpointId>, SessionId);

/// What a machine says when it has no child of that name for the caller: also
/// what stopping one that is already gone reads as success on.
pub(crate) const NO_AGENT: &str = "You have no agent";

/// What the node knows of the family trees it takes part in.
#[derive(Default)]
pub(crate) struct Links {
    pub(super) remote: Arc<Mutex<Vec<RemoteKid>>>,
    /// The mesh the node belongs to, once it does.
    pub(super) membership: Membership,
    /// Per parent with children elsewhere, the task that stops them when it
    /// ends for good.
    pub(super) watchers: Mutex<HashMap<SessionId, AbortOnDropHandle<()>>>,
    /// The child starts under way here, each under its root. They count
    /// with the root's live agents, so two starts cannot both take the last
    /// place.
    pub(super) starting: Mutex<Vec<(Root, Weak<()>)>>,
    /// What transcripts show of children on other machines.
    pub(crate) views: Views,
    /// The children parents wait on in `start_agent`, which send no note
    /// meanwhile.
    pub(crate) waits: Waits,
}

impl Links {
    pub(crate) fn new(membership: Membership) -> Self {
        Self {
            membership,
            ..Self::default()
        }
    }

    /// The mesh, when this node belongs to one.
    pub(crate) fn mesh(&self) -> Option<beep_mesh::Mesh> {
        self.membership.get()
    }

    /// This machine's id in its mesh.
    pub(in crate::agents) fn me(&self) -> Option<EndpointId> {
        self.mesh().map(|mesh| mesh.me())
    }

    /// Whether `kid` is one `parent` keeps on `machine`: by the id its machine
    /// lists it under or the one it had when it started.
    pub(super) fn keeps(&self, parent: &SessionId, machine: EndpointId, id: &SessionId, key: &SessionId) -> bool {
        crate::lock(&self.remote).iter().any(|kid| {
            kid.parent == *parent
                && kid.machine == machine
                && kid.counts()
                && (kid.pending() || kid.id == *id || kid.id == *key)
        })
    }

    /// The links the open sessions keep over a restart: the remote children
    /// of the session `name`.
    pub(in crate::agents) fn kids_to_keep(&self, name: &SessionId) -> Vec<RemoteKid> {
        crate::lock(&self.remote)
            .iter()
            .filter(|kid| kid.parent == *name && !kid.pending())
            .cloned()
            .collect()
    }
}

/// Who calls a family tool: a session of some machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Who {
    /// `None` for a node that belongs to no mesh.
    pub(crate) machine: Option<EndpointId>,
    /// The session's id as its children's links name it.
    pub(crate) session: SessionId,
}

/// What a child starts with, from the call that starts it.
#[derive(Debug)]
pub(crate) struct ChildStart {
    pub(crate) harness: String,
    pub(crate) prompt: String,
    pub(crate) cwd: Option<String>,
    pub(crate) model: Option<ModelId>,
    /// A temporary reviewer, started in its harness's read-only mode.
    pub(crate) review: Option<Review>,
    /// The named account its parent runs as. A child of an agent that keeps
    /// accounts runs as that agent's account of the same name, or not at all.
    pub(crate) account: Option<AccountName>,
    /// The name its parent gives it, which its chat lists it under.
    pub(crate) title: Option<String>,
    /// The custom agent it runs as, whose instructions it is told.
    pub(crate) persona: Option<CustomAgent>,
}

/// What a reviewer child starts with besides `ChildStart`'s.
#[derive(Debug, Default)]
pub(crate) struct Review {
    pub(crate) effort: Option<Effort>,
    pub(crate) fast: Option<bool>,
}

/// Whether `live` is a child of `who`.
pub(in crate::agents) fn is_child_of(live: &Live, who: &Who) -> bool {
    live.info
        .parent
        .as_ref()
        .is_some_and(|link| link.session == who.session && link.machine == who.machine)
}

/// How long a child whose start got no answer counts, while its parent's node
/// cannot tell whether it runs.
pub(super) const PENDING_FOR: Duration = Duration::from_secs(120);

/// A child that runs on another machine, as its parent's node keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RemoteKid {
    /// The parent's id as its children's links name it.
    pub(crate) parent: SessionId,
    pub(crate) machine: EndpointId,
    /// The machine as agents name it in `list_machines`.
    pub(crate) name: String,
    /// The child's id on that machine; empty while the order to start it has
    /// no answer yet, or got none.
    pub(crate) id: SessionId,
    /// It ended or was stopped: it stays readable but no longer counts.
    #[serde(default)]
    pub(crate) ended: bool,
    /// When the node noted it, in Unix seconds.
    #[serde(default)]
    pub(crate) since: u64,
}

impl RemoteKid {
    /// A child the order to start is on its way for.
    pub(crate) fn expected(parent: &SessionId, machine: EndpointId, name: &str) -> Self {
        Self {
            parent: parent.clone(),
            machine,
            name: name.to_owned(),
            id: SessionId::default(),
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

/// What a child came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::AsRefStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub(crate) enum NoteState {
    /// Its turn ended with nothing queued behind it.
    Finished,
    /// It waits on the user.
    Asking,
    /// The user stopped its turn.
    Stopped,
    /// The user closed its session.
    Closed,
    /// It settled without news for the parent: its session ended, a turn
    /// the parent stopped, or its last background work. Only wakes a wait,
    /// and goes in no prompt. A parent's node that does not know it refuses
    /// the note.
    Settled,
}

/// What a child's node tells the parent's: JSON in an `AgentNote` call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ChildNote {
    /// The child's id on its machine, as its parent got it.
    pub(crate) id: SessionId,
    /// Its id on its machine from before the agent gave its own.
    #[serde(default)]
    pub(crate) key: SessionId,
    /// Its title, else its harness.
    #[serde(default)]
    pub(crate) name: String,
    pub(crate) state: NoteState,
    /// Its last message when it finished, its question when it asks.
    #[serde(default)]
    pub(crate) text: String,
}

impl ChildNote {
    /// The note as the parent's agent reads it, about the child it knows as
    /// `id`.
    pub(crate) fn text_for(&self, id: &str) -> String {
        let (head, tail) = match self.state {
            NoteState::Finished => (
                format!("[child {id} finished] {}", self.text),
                "Reply with send_to_agent to continue it, or stop it with stop_agent once you \
                 need nothing more from it.",
            ),
            NoteState::Asking => (
                format!("[child {id} is waiting on the user] {}", self.text),
                "Only the user can answer this.",
            ),
            NoteState::Stopped => (
                format!("[child {id} was stopped by the user] {}", self.text),
                "Reply with send_to_agent to continue it.",
            ),
            NoteState::Closed => (
                format!("[child {id} was stopped by the user] {}", self.text),
                "Its session ended.",
            ),
            NoteState::Settled => (format!("[child {id} settled] {}", self.text), ""),
        };
        let body = format!("{}\n{tail}", head.trim_end());
        tags::from_child(id, &self.name, self.state.as_ref(), &body)
    }
}

/// The children each parent waits on in `start_agent`, by the parent's name
/// and the child's id as the parent got it, each with what wakes its wait. Parallel waits on one child each have their own.
#[derive(Clone, Default)]
pub(crate) struct Waits(Arc<Mutex<HashMap<WaitKey, Vec<Arc<Notify>>>>>);

/// A parent's name and the child's id as the parent got it.
type WaitKey = (SessionId, String);

/// Held while a parent waits on a child; the wait ends when it drops.
pub(crate) struct Waiting {
    waits: Waits,
    key: WaitKey,
    woken: Arc<Notify>,
}

impl Waiting {
    /// Ready once the child's node pushed a note about it. A note that came
    /// before this is asked for still counts.
    pub(crate) async fn woken(&self) {
        self.woken.notified().await;
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        let mut held = crate::lock(&self.waits.0);

        if let Some(all) = held.get_mut(&self.key) {
            all.retain(|one| !Arc::ptr_eq(one, &self.woken));

            if all.is_empty() {
                held.remove(&self.key);
            }
        }
    }
}

impl Waits {
    pub(crate) fn hold(&self, parent: &SessionId, child: &str) -> Waiting {
        let key = (parent.to_owned(), child.to_owned());
        let woken = Arc::new(Notify::new());
        crate::lock(&self.0)
            .entry(key.clone())
            .or_default()
            .push(woken.clone());
        Waiting {
            waits: self.clone(),
            key,
            woken,
        }
    }

    /// Wakes the wait of `parent` on `child`, if any. The wait answers the
    /// note, so it goes in no prompt.
    pub(super) fn wake(&self, parent: &SessionId, child: &str) -> bool {
        let held = crate::lock(&self.0);

        let Some(all) = held.get(&(parent.to_owned(), child.to_owned())) else {
            return false;
        };

        for woken in all {
            woken.notify_one();
        }

        true
    }
}

/// The last state read of each child on another machine, for transcripts: a
/// read of a transcript shows what is known and never waits on a machine,
/// which may be off. A state older than `VIEW_FRESH` is read again in the
/// background, for the next read.
#[derive(Clone, Default)]
pub(crate) struct Views(pub(super) Arc<Mutex<HashMap<(EndpointId, SessionId), Seen>>>);

#[derive(Default)]
pub(super) struct Seen {
    pub(super) view: Option<ChildView>,
    /// When the last read ended, well or not.
    pub(super) at: Option<Instant>,
    pub(super) reading: Option<AbortOnDropHandle<()>>,
}
