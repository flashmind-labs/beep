//! Tells a parent when its child ends a turn, starts waiting on the user or
//! is stopped by the user, so it need not poll: a note goes into the
//! parent's session, as a steer while it works and as a prompt when it is
//! idle.
//!
//! The child's node follows the child's state. A parent here gets the note
//! at once; one on another machine gets it over `tools/1` (`AgentNote`),
//! and its node checks that it keeps that child. No note goes to a parent
//! that ended or was stopped, for a turn or session the parent stopped, or
//! while the parent waits on that child in `start_agent`: the note ends
//! that wait, which then gives the state. Reviewers report through `submit_review` and send none.
//!
//! What the user did in the child is no news that should start a turn: the
//! end of a turn the user's message began, a question in it, and a stop or a
//! close by the user send nothing to an idle parent here, whose next turn
//! hears of it instead (`changes`). A parent on another machine is told as
//! before.

use crate::agents::sessions;
use beep_proto::SessionId;
use super::links::{ChildNote, NoteState, Waits};
use crate::agents::{Agents, Session, live::Live, session::Command};
use crate::agents::live::Mark;
use crate::agents::child_view::{ChildView, full_id};
use super::peer::NoteFailed;
use crate::membership::Membership;
use beep_proto::node::agent::{Status, StopReason};
use beep_proto::watched;
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{
    future::Future,
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::{
    sync::watch,
    time::sleep,
};

/// What a text handed to a session is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Delivery {
    /// A teammate's agent asks: a message, which counts against the
    /// session's messages from agents.
    Ask,
    /// The answer to the session's own ask: a note.
    Reply,
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
            Some(peer) if Some(peer) != self.links.me() => To::There {
                peer,
                parent: link.session,
            },
            _ => match self.sessions.get(&link.session) {
                Some(parent) => To::Here {
                    parent: Arc::downgrade(&parent),
                    waits: self.links.waits.clone(),
                },
                None => return,
            },
        };
        let child = Child {
            id: session.id.clone(),
            live: session.live.clone(),
            session: Arc::downgrade(session),
        };
        let task = follow(child, to, self.links.membership.clone());
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
            .links.remote_kids(parent)
            .into_iter()
            .find(|kid| {
                kid.machine == peer && !kid.ended && (kid.id == note.id || kid.id == note.key)
            })
            .ok_or_else(|| format!("{} {}.", super::NO_AGENT, note.id))?;
        let session = self
            .sessions
            .get(parent)
            .ok_or_else(|| format!("There is no session {parent} here."))?;
        let id = format!("{}@{}", kid.id, kid.name);
        deliver(&session, &self.links.waits, &id, note, None).await
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
    pub(crate) fn teammates_session(&self, id: &SessionId) -> Option<Arc<Session>> {
        self.sessions.peers(None)
            .into_iter()
            .find(|session| session.id == *id || sessions::name_of(session) == *id)
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

/// The child `follow` follows.
struct Child {
    /// Its id on this node, which its parent here keeps what it saw of it by.
    id: SessionId,
    live: watch::Receiver<Live>,
    /// Whom `Session::take_user_turn` asks whether the user's message began
    /// its turn. Weak: the session keeps this task.
    session: Weak<Session>,
}

/// Follows a child's state and sends a note for each turn it finishes, each
/// time it starts to ask and each stop by the user, until it ends.
async fn follow(child: Child, to: To, membership: Membership) {
    let Child {
        id: key,
        mut live,
        session,
    } = child;
    // A session counts its turns from its start, so one that ended before
    // this first look is told too.
    let (mut turns, mut asking) = (0, false);
    // Whether the child looked settled to its parent, which a parent on
    // another machine waits on; one settling at the first look is not news.
    let (remote, mut was_settled) = (matches!(to, To::There { .. }), true);
    let mut first = true;

    while std::mem::take(&mut first) || live.changed().await.is_ok() {
        let mut ended = false;
        let mut quiet = false;
        // The turn it ends is the user's if so marked, and the mark ends with it.
        let mut take = false;
        let told = watched::read_and_update(&mut live, |now| {
            let note = match now.status() {
                Status::Ended => {
                    ended = true;
                    quiet = true;
                    now.stopped_by_user.then(|| note_of(now, NoteState::Closed))
                }
                Status::Asking if !asking => {
                    asking = true;
                    quiet = now.user_turn;
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
                        take = true;
                        now.stopped_by_user
                            .then(|| note_of(now, NoteState::Stopped))
                    } else if now.info.state.queued == 0 {
                        take = true;
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

            let settled = ChildView::of(&now.info, &SessionId::default()).settled();
            let note = note.or_else(|| {
                (remote && settled && !was_settled).then(|| note_of(now, NoteState::Settled))
            });
            was_settled = settled;

            note.map(|note| Told {
                note,
                mark: Into::<Mark>::into(&*now),
                quiet,
            })
        });
        let user = take && match session.upgrade() {
            Some(session) => session.take_user_turn().await,
            None => false,
        };
        let told = told.map(|told| Told {
            quiet: told.quiet || user,
            ..told
        });
        let Some(Told { note, mark, quiet }) = told else {
            if ended {
                return;
            }
            continue;
        };
        let sent = match &to {
            To::Here { parent, waits } => match parent.upgrade() {
                Some(parent) => {
                    let mesh = membership.get();
                    let id = full_id(note.id.as_ref(), &crate::agents::machine_list::machine_name(mesh.as_ref()));
                    let seen = Seen {
                        child: &key,
                        mark,
                        quiet,
                    };
                    deliver(&parent, waits, &id, &note, Some(seen)).await
                }
                None => return,
            },
            To::There { peer, parent } => {
                tell_until_heard(|| async {
                    match membership.get() {
                        Some(mesh) => super::peer::note_parent(&mesh, *peer, parent, &note).await,
                        None => Err(NoteFailed::Silent("this machine is in no mesh".to_owned())),
                    }
                })
                .await
            }
        };

        if let Err(e) = sent {
            log::info!("the parent of child {} was not told: {e}", note.id);
        }

        if ended {
            return;
        }
    }
}

/// How long a note that got no answer waits before it is sent again.
const NOTE_RETRY: Duration = Duration::from_secs(5);

/// Sends a note with `send` until the parent's node answers: a note that
/// went unheard is the only news a waiting parent gets of a settle, so it is
/// not dropped. A node that answers and refuses is not asked again.
pub(in crate::agents) async fn tell_until_heard<F>(send: impl Fn() -> F) -> Result<(), String>
where
    F: Future<Output = Result<(), NoteFailed>>,
{
    loop {
        match send().await {
            Ok(()) => return Ok(()),
            Err(NoteFailed::Refused(why)) => return Err(why),
            Err(NoteFailed::Silent(why)) => log::info!("a parent was not told, trying again: {why}"),
        }

        sleep(NOTE_RETRY).await;
    }
}

/// What the child `live` came to, as its node tells it.
fn note_of(live: &Live, state: NoteState) -> ChildNote {
    let view = ChildView::of(&live.info, &SessionId::default());
    let text = match state {
        NoteState::Finished if view.last_message.is_empty() => view.error.unwrap_or_default(),
        NoteState::Finished | NoteState::Stopped | NoteState::Closed | NoteState::Settled => {
            view.last_message
        }
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
    let (idle, stopped) = watched::read(&parent.live, idle_or_stopped);

    if stopped {
        return Ok(());
    }

    if waits.wake(&sessions::name_of(parent), id) || note.state == NoteState::Settled {
        return Ok(());
    }

    if let Some(seen) = seen {
        if seen.quiet && idle {
            return Ok(());
        }

        parent.see(vec![(seen.child.clone(), seen.mark)]).await;
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
