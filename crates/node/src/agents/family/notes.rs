//! Tells a parent when its child ends a turn, starts waiting on the user or
//! is stopped by the user, so it need not poll: a note goes into the
//! parent's session, as a steer while it works and as a prompt when it is
//! idle.
//!
//! The child's node follows the child's state. A parent here gets the note
//! at once; one on another machine gets it over `tools/1` (`AgentNote`),
//! and its node checks that it keeps that child. No note goes to a parent
//! that ended or was stopped, for a turn or session the parent stopped, or
//! while the parent waits on that child in `start_agent` (`wait` gives the
//! state). Reviewers report through `submit_review` and send none.

use beep_proto::SessionId;
use super::{Agents, Command, Live, Session, children};
use crate::{agents::tags, link::Membership, team::asks::Delivery};
use beep_proto::node::agent::{Status, StopReason};
use beep_proto::watched;
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex, OnceLock, Weak},
};
use tokio::sync::watch;

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
        };
        let body = format!("{}\n{tail}", head.trim_end());
        tags::from_child(id, &self.name, self.state.as_ref(), &body)
    }
}

/// The children each parent waits on in `start_agent`, by the parent's name
/// and the child's id as the parent got it.
#[derive(Clone, Default)]
pub(crate) struct Waits(Arc<Mutex<HashSet<(SessionId, String)>>>);

/// Held while a parent waits on a child; the wait ends when it drops.
pub(crate) struct Waiting {
    waits: Waits,
    key: (SessionId, String),
}

impl Drop for Waiting {
    fn drop(&mut self) {
        crate::lock(&self.waits.0).remove(&self.key);
    }
}

impl Waits {
    pub(crate) fn hold(&self, parent: &SessionId, child: &str) -> Waiting {
        let key = (parent.to_owned(), child.to_owned());
        crate::lock(&self.0).insert(key.clone());
        Waiting {
            waits: self.clone(),
            key,
        }
    }

    fn holds(&self, parent: &SessionId, child: &str) -> bool {
        crate::lock(&self.0).contains(&(parent.to_owned(), child.to_owned()))
    }
}

/// Where a child's notes go.
enum To {
    /// The parent runs here.
    Here { parent: Weak<Session>, waits: Waits },
    /// The parent runs on `peer`, under the name `parent`.
    There { peer: EndpointId, parent: SessionId },
}

impl Agents {
    /// Has `session`, a child, tell its parent when it finishes a turn or
    /// asks the user, for as long as it runs.
    pub(in crate::agents) fn tell_parent(&self, session: &Arc<Session>) {
        if session.reviewer {
            return;
        }

        let Some(link) = session.info().parent else {
            return;
        };
        let to = match link.machine {
            Some(peer) if Some(peer) != self.me() => To::There {
                peer,
                parent: link.session,
            },
            _ => match self.get(&link.session) {
                Some(parent) => To::Here {
                    parent: Arc::downgrade(&parent),
                    waits: self.family.waits.clone(),
                },
                None => return,
            },
        };
        let task = follow(session.live.clone(), to, self.family.membership.clone());
        session.keep(tokio::spawn(task));
    }

    /// A note from `peer`'s child of this node's session `parent`: delivered
    /// when the parent keeps that child.
    pub(crate) async fn note_from(
        &self,
        peer: EndpointId,
        parent: &SessionId,
        note: &ChildNote,
    ) -> Result<(), String> {
        let kid = self
            .remote_kids(parent)
            .into_iter()
            .find(|kid| {
                kid.machine == peer && !kid.ended && (kid.id == note.id || kid.id == note.key)
            })
            .ok_or_else(|| format!("{} {}.", super::NO_AGENT, note.id))?;
        let session = self
            .get(parent)
            .ok_or_else(|| format!("There is no session {parent} here."))?;
        let id = format!("{}@{}", kid.id, kid.name);
        deliver(&session, &self.family.waits, &id, note).await
    }
}

impl Agents {
    /// Puts `text` from a teammate's agent into the live session `id` here,
    /// which the team knows as `card`: an ask as a message from an agent, an
    /// answer as a note, the way a child's notes go in. Neither goes in while
    /// the session waits on its user, who alone answers it there, nor into
    /// a session the user stopped.
    pub(crate) async fn deliver_from_teammate(
        &self,
        id: &SessionId,
        card: &str,
        delivery: Delivery,
        text: String,
    ) -> Result<(), String> {
        let session = self
            .teammates_session(id)
            .ok_or_else(|| format!("{card} no longer runs"))?;
        let idle = watched::read(&session.live, |live| open_to_teammates(live, card))?;

        match delivery {
            Delivery::Ask => self.message(&SessionId::default(), id, text).await,
            Delivery::Reply => put(&session, idle, text).await,
        }
    }

    /// The live session here a team knows by `id`: its own id or its agent's.
    pub(in crate::agents) fn teammates_session(&self, id: &SessionId) -> Option<Arc<Session>> {
        self.peers(None)
            .into_iter()
            .find(|session| session.id == *id || Self::name_of(session) == *id)
    }
}

/// Whether `live`, the session a team knows as `card`, is idle; or why a
/// teammate's words may not go in now.
pub(super) fn open_to_teammates(live: &Live, card: &str) -> Result<bool, String> {
    let (idle, stopped) = idle_or_stopped(live);

    if live.status() == Status::Asking {
        return Err(format!("{card} is waiting on its user"));
    }
    if stopped {
        return Err(format!("{card} was stopped by its user"));
    }

    Ok(idle)
}

/// Follows a child's state and sends a note for each turn it finishes, each
/// time it starts to ask and each stop by the user, until it ends.
async fn follow(mut live: watch::Receiver<Live>, to: To, membership: Arc<OnceLock<Membership>>) {
    // A session counts its turns from its start, so one that ended before
    // this first look is told too.
    let (mut turns, mut asking) = (0, false);
    let mut first = true;

    while std::mem::take(&mut first) || live.changed().await.is_ok() {
        let mut ended = false;
        let note = watched::read_and_update(&mut live, |now| match now.status() {
            Status::Ended => {
                ended = true;
                now.stopped_by_user.then(|| note_of(now, NoteState::Closed))
            }
            Status::Asking if !asking => {
                asking = true;
                Some(note_of(now, NoteState::Asking))
            }
            Status::Asking => None,
            Status::Idle => {
                asking = false;
                let done = now.info.turns > turns;
                turns = now.info.turns;
                // A turn with prompts behind it is not the end, and the
                // parent knows of a stop it made itself.
                if !done {
                    None
                } else if now.info.state.last_stop == Some(StopReason::Cancelled) {
                    now.stopped_by_user
                        .then(|| note_of(now, NoteState::Stopped))
                } else {
                    (now.info.state.queued == 0).then(|| note_of(now, NoteState::Finished))
                }
            }
            _ => {
                asking = false;
                None
            }
        });
        let Some(note) = note else {
            if ended {
                return;
            }
            continue;
        };
        let mesh = membership.get().and_then(Membership::get);
        let sent = match &to {
            To::Here { parent, waits } => match parent.upgrade() {
                Some(parent) => {
                    let id = children::full_id(note.id.as_ref(), &children::machine_name(mesh.as_ref()));
                    deliver(&parent, waits, &id, &note).await
                }
                None => return,
            },
            To::There { peer, parent } => match &mesh {
                Some(mesh) => children::note_parent(mesh, *peer, parent, &note).await,
                None => Err("this machine is in no mesh".to_owned()),
            },
        };

        if let Err(e) = sent {
            log::info!("the parent of child {} was not told: {e}", note.id);
        }

        if ended {
            return;
        }
    }
}

/// What the child `live` came to, as its node tells it.
fn note_of(live: &Live, state: NoteState) -> ChildNote {
    let view = children::ChildView::of(&live.info, &SessionId::default());
    let text = match state {
        NoteState::Finished if view.last_message.is_empty() => view.error.unwrap_or_default(),
        NoteState::Finished | NoteState::Stopped | NoteState::Closed => view.last_message,
        NoteState::Asking => view.question.unwrap_or_default(),
    };
    ChildNote {
        id: live
            .agent_session
            .clone()
            .unwrap_or_else(|| live.info.id.clone()),
        key: live.info.id.clone(),
        name: live
            .info
            .title
            .clone()
            .unwrap_or_else(|| live.info.agent.clone()),
        state,
        text,
    }
}

/// Puts `note`, about the child `parent` knows as `id`, into `parent`: none
/// when the parent ended or was stopped, or waits on that child.
async fn deliver(
    parent: &Session,
    waits: &Waits,
    id: &str,
    note: &ChildNote,
) -> Result<(), String> {
    let (idle, stopped) = watched::read(&parent.live, idle_or_stopped);

    if stopped || waits.holds(&Agents::name_of(parent), id) {
        return Ok(());
    }

    put(parent, idle, note.text_for(id)).await
}

/// Whether a session is idle, and whether it ended or the user stopped it.
fn idle_or_stopped(live: &Live) -> (bool, bool) {
    let status = live.status();
    let stopped = status == Status::Ended
        || (status == Status::Idle && live.info.state.last_stop == Some(StopReason::Cancelled));

    (status == Status::Idle, stopped)
}

/// Puts `text` into `session`: a prompt when it is `idle`, else a steer.
async fn put(session: &Session, idle: bool, text: String) -> Result<(), String> {
    let text = text.into();
    let command = if idle {
        Command::Prompt(text)
    } else {
        Command::Steer(text)
    };
    session.order(command).await
}
