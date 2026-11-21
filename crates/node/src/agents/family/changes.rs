//! What a session of a chat hears of the others when the user writes to it:
//! which threads changed since its last turn, and for a thread what the main
//! chat said last. Nobody is prompted for it, so an idle session stays idle;
//! the words go ahead of the user's own, as Beep's context.
//!
//! A session's relatives are its threads, its main chat and that chat's
//! other threads, on this machine. What it last saw of each is kept while
//! the node runs: after a restart each thread is told of once more.

use super::{Agents, Command, Live, Session, children};
use crate::agents::{acp::Prompt, cut, tags};
use beep_proto::node::agent::{SessionInfo, Status, StopReason};
use beep_proto::watched;
use std::sync::{Arc, atomic::Ordering};

/// How the context starts.
const HEAD: &str = "Since your last turn in this chat, as context and not a request \
    (read_session_transcript reads more):";
/// The most of a relative's last message or question a line holds.
const MAX_TEXT: usize = 300;

/// How a relative stands, as the context says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::AsRefStr)]
#[strum(serialize_all = "snake_case")]
enum Stand {
    Working,
    WaitingOnUser,
    Idle,
    /// Idle since the user stopped its turn.
    Stopped,
    Ended,
}

impl From<&Live> for Stand {
    fn from(live: &Live) -> Self {
        let state = &live.info.state;

        match state.status {
            Status::Ended => Self::Ended,
            Status::Asking => Self::WaitingOnUser,
            Status::Idle if live.stopped_by_user && state.last_stop == Some(StopReason::Cancelled) => {
                Self::Stopped
            }
            Status::Idle if state.subagents == 0 => Self::Idle,
            _ => Self::Working,
        }
    }
}

/// What a session saw of a relative: told again once it differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::agents) struct Mark {
    turns: u32,
    stand: Stand,
}

impl From<&Live> for Mark {
    fn from(live: &Live) -> Self {
        Self {
            turns: live.info.turns,
            stand: Stand::from(live),
        }
    }
}

/// A child the user sees as a thread of its parent's chat: no reviewer and
/// no background agent.
pub(super) fn is_thread(info: &SessionInfo) -> bool {
    !info.reviewer && info.parent.as_ref().is_some_and(|link| !link.background)
}

impl Agents {
    /// What the user sent `session`: a message counts as the user's, and
    /// takes what changed among the session's relatives ahead of it.
    pub(in crate::agents) fn by_user(&self, session: &Arc<Session>, command: Command) -> Command {
        match command {
            Command::Prompt(prompt) => Command::Prompt(self.heard(session, prompt)),
            Command::Steer(prompt) => Command::Steer(self.heard(session, prompt)),
            other => other,
        }
    }

    fn heard(&self, session: &Arc<Session>, mut prompt: Prompt) -> Prompt {
        session.user_wrote();
        let status = watched::read(&session.live, Live::status);

        // A message to an idle session starts a turn that is the user's.
        if status == Status::Idle {
            session.user_turn.store(true, Ordering::Relaxed);
        }

        // Words sent while it asks may be the answer, which takes nothing else.
        if status != Status::Asking
            && let Some(changes) = self.changes(session)
        {
            prompt.text = format!("{changes}\n\n{}", prompt.text);
        }

        prompt
    }

    /// What changed among `session`'s relatives here since it last saw
    /// them, which it has now seen.
    fn changes(&self, session: &Arc<Session>) -> Option<String> {
        let me = self.who_of(session);
        let main = session
            .info()
            .parent
            .filter(|link| link.machine == me.machine)
            .and_then(|link| self.get(&link.session));
        let mut lines = Vec::new();

        if let Some(main) = &main {
            let (mark, said) = watched::read(&main.live, |live| {
                (Into::<Mark>::into(live), one_line(&live.info.last_message))
            });
            let before = session.see(&main.id, mark);

            if before.is_none_or(|before| before.turns != mark.turns) && !said.is_empty() {
                lines.push(format!("- main chat: {said}"));
            }
        }

        let here = children::machine_name(self.mesh().as_ref());
        let siblings = main
            .iter()
            .flat_map(|main| self.children_of(&self.who_of(main)))
            .filter(|other| !Arc::ptr_eq(other, session));

        for thread in self.children_of(&me).into_iter().chain(siblings) {
            let Some((mark, line)) = watched::read(&thread.live, |live| {
                is_thread(&live.info).then(|| (Into::<Mark>::into(live), line(live, &here)))
            }) else {
                continue;
            };
            let news = match session.see(&thread.id, mark) {
                Some(before) => before != mark,
                // One never seen that has yet to do anything says nothing.
                None => mark.turns > 0 || mark.stand != Stand::Working,
            };

            if news {
                lines.push(line);
            }
        }

        (!lines.is_empty()).then(|| tags::context(&format!("{HEAD}\n{}", lines.join("\n"))))
    }
}

/// The line for the thread `live`, on the machine agents call `here`.
fn line(live: &Live, here: &str) -> String {
    let info = &live.info;
    let id = live.agent_session.as_ref().unwrap_or(&info.id);
    let id = children::full_id(id.as_ref(), here);
    let name = info.title.as_deref().unwrap_or(&info.agent);
    let stand = Stand::from(live);
    let text = match &info.question {
        Some(question) if stand == Stand::WaitingOnUser => one_line(&question.text),
        _ => one_line(&info.last_message),
    };
    let stand = stand.as_ref();

    if text.is_empty() {
        format!("- thread \"{name}\" {id}, {stand}")
    } else {
        format!("- thread \"{name}\" {id}, {stand}: {text}")
    }
}

/// `text` on one line, cut short.
fn one_line(text: &str) -> String {
    cut(&text.split_whitespace().collect::<Vec<_>>().join(" "), MAX_TEXT)
}
