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
//!
//! What the user did in the child is no news that should start a turn: the
//! end of a turn the user's message began, a question in it, and a stop or a
//! close by the user send nothing to an idle parent here, whose next turn
//! hears of it instead (`changes`). A parent on another machine is told as
//! before.

use beep_proto::SessionId;
use super::{Agents, Command, Live, Mark, Session, children};
use crate::{agents::tags, link::Membership};
use beep_proto::node::agent::{Status, StopReason};
use beep_proto::watched;
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
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

/// A note and how the child stood when it was made.
struct Told {
    note: ChildNote,
    mark: Mark,
    /// The user's doing, which wakes no idle parent.
    quiet: bool,
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
        let child = Child {
            id: session.id.clone(),
            live: session.live.clone(),
            user_turn: session.user_turn.clone(),
        };
        let task = follow(child, to, self.family.membership.clone());
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
        deliver(&session, &self.family.waits, &id, note, None).await
    }
}

/// The child `follow` follows.
struct Child {
    /// Its id on this node, which its parent here keeps what it saw of it by.
    id: SessionId,
    live: watch::Receiver<Live>,
    /// Whether the user's message began its turn (`Session::user_turn`).
    user_turn: Arc<AtomicBool>,
}

/// Follows a child's state and sends a note for each turn it finishes, each
/// time it starts to ask and each stop by the user, until it ends.
async fn follow(child: Child, to: To, membership: Arc<OnceLock<Membership>>) {
    let Child {
        id: key,
        mut live,
        user_turn,
    } = child;
    // A session counts its turns from its start, so one that ended before
    // this first look is told too.
    let (mut turns, mut asking) = (0, false);
    let mut first = true;

    while std::mem::take(&mut first) || live.changed().await.is_ok() {
        let mut ended = false;
        let mut quiet = false;
        let told = watched::read_and_update(&mut live, |now| {
            let note = match now.status() {
                Status::Ended => {
                    ended = true;
                    quiet = true;
                    now.stopped_by_user.then(|| note_of(now, NoteState::Closed))
                }
                Status::Asking if !asking => {
                    asking = true;
                    quiet = user_turn.load(Ordering::Relaxed);
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
                        quiet = true;
                        user_turn.store(false, Ordering::Relaxed);
                        now.stopped_by_user
                            .then(|| note_of(now, NoteState::Stopped))
                    } else if now.info.state.queued == 0 {
                        quiet = user_turn.swap(false, Ordering::Relaxed);
                        Some(note_of(now, NoteState::Finished))
                    } else {
                        None
                    }
                }
                _ => {
                    asking = false;
                    None
                }
            };

            note.map(|note| Told {
                note,
                mark: Into::<Mark>::into(&*now),
                quiet,
            })
        });
        let Some(Told { note, mark, quiet }) = told else {
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
                    let seen = Seen {
                        child: &key,
                        mark,
                        quiet,
                    };
                    deliver(&parent, waits, &id, &note, Some(seen)).await
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

/// What a parent here is to note of the child a note is about.
struct Seen<'a> {
    /// The child's id on this node.
    child: &'a SessionId,
    /// How it stood when the note was made.
    mark: Mark,
    /// The note is of the user's doing.
    quiet: bool,
}

/// Puts `note`, about the child `parent` knows as `id`, into `parent`: none
/// when the parent ended or was stopped, or waits on that child, and none of
/// the user's doing while the parent is idle, which stays unseen for the
/// parent's next turn to say. `seen` is of a child on this node.
async fn deliver(
    parent: &Session,
    waits: &Waits,
    id: &str,
    note: &ChildNote,
    seen: Option<Seen<'_>>,
) -> Result<(), String> {
    let (idle, stopped) = {
        watched::read(&parent.live, |live| {
            let status = live.status();
            let stopped = status == Status::Ended
                || (status == Status::Idle && live.info.state.last_stop == Some(StopReason::Cancelled));
            (status == Status::Idle, stopped)
        })
    };

    if stopped || waits.holds(&Agents::name_of(parent), id) {
        return Ok(());
    }

    if let Some(seen) = seen {
        if seen.quiet && idle {
            return Ok(());
        }

        parent.see(seen.child, seen.mark);
    }

    let text = note.text_for(id).into();
    let command = if idle {
        Command::Prompt(text)
    } else {
        Command::Steer(text)
    };
    parent.order(command).await
}
