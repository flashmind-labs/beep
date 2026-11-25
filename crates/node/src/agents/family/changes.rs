//! What a session of a chat hears of the others when the user writes to it:
//! which threads changed since its last turn, and for a thread what the main
//! chat said last. Nobody is prompted for it, so an idle session stays idle;
//! the words go with the user's message as its context, apart from its text.
//!
//! A session's relatives are its threads, its main chat and that chat's
//! other threads, on this machine. What it last saw of each is kept while
//! the node runs: after a restart each thread is told of once more.

use super::{Agents, Command, Live, Session};
use crate::agents::child_view::full_id;
use crate::agents::live::{Mark, Stand};
use crate::agents::{acp::Prompt, cut};
use beep_proto::SessionId;
use beep_proto::node::agent::{SessionInfo, Status};
use beep_proto::watched;
use std::sync::Arc;

/// How the context starts.
const HEAD: &str = "Since your last turn in this chat, as context and not a request \
    (read_session_transcript reads more):";
/// The most of a relative's last message or question a line holds.
const MAX_TEXT: usize = 300;

/// A child the user sees as a thread of its parent's chat: no reviewer and
/// no background agent.
pub(super) fn is_thread(info: &SessionInfo) -> bool {
    !info.reviewer && info.parent.as_ref().is_some_and(|link| !link.background)
}

/// What a session's relatives came to, and how each stood when that was
/// read: seen once the session took the message that says so.
#[derive(Default)]
pub(super) struct News {
    text: Option<String>,
    seen: Vec<(SessionId, Mark)>,
}

impl Agents {
    /// Orders what the user sent `session`: a message counts as the user's,
    /// and takes what changed among the session's relatives ahead of it. A
    /// message the session refuses leaves that for the next.
    pub(in crate::agents) async fn order_by_user(
        &self,
        session: &Arc<Session>,
        command: Command,
    ) -> Result<(), String> {
        let (command, news) = match command {
            Command::Prompt(prompt) => {
                let (prompt, news) = self.heard(session, prompt).await;
                (Command::Prompt(prompt), news)
            }
            Command::Steer(prompt) => {
                let (prompt, news) = self.heard(session, prompt).await;
                (Command::Steer(prompt), news)
            }
            other => (other, News::default()),
        };
        session.order(command).await?;

        session.see(news.seen).await;

        Ok(())
    }

    /// The user's `prompt` to `session` with what its relatives came to,
    /// which it has not seen until it takes the prompt.
    pub(super) async fn heard(&self, session: &Arc<Session>, mut prompt: Prompt) -> (Prompt, News) {
        session.user_wrote();
        let status = watched::read(&session.live, Live::status);

        // A message to an idle session starts a turn that is the user's.
        if status == Status::Idle {
            session.set_user_turn(true).await;
        }

        // Words sent while it asks may be the answer, and a command is the
        // whole message: neither takes anything else, so the news waits.
        if status == Status::Asking || prompt.text.trim_start().starts_with('/') {
            return (prompt, News::default());
        }

        let news = self.changes(session);
        prompt.context.clone_from(&news.text);
        (prompt, news)
    }

    /// What changed among `session`'s relatives here since it last saw
    /// them.
    fn changes(&self, session: &Arc<Session>) -> News {
        let me = self.who_of(session);
        let main = session
            .info()
            .parent
            .filter(|link| link.machine == me.machine)
            .and_then(|link| self.get(&link.session));
        let mut lines = Vec::new();
        let mut seen = Vec::new();

        if let Some(main) = &main {
            let (mark, said) = watched::read(&main.live, |live| {
                (Into::<Mark>::into(live), one_line(&live.info.last_message))
            });
            let before = session.saw(&main.id);
            seen.push((main.id.clone(), mark));

            if before.is_none_or(|before| before.turns != mark.turns) && !said.is_empty() {
                lines.push(format!("- parent session: {said}"));
            }
        }

        let here = crate::agents::machine_list::machine_name(self.mesh().as_ref());
        let siblings = main
            .iter()
            .flat_map(|main| self.children_of(&self.who_of(main)))
            .filter(|other| !Arc::ptr_eq(other, session));

        for thread in self.children_of(&me).into_iter().chain(siblings) {
            // A reviewer is known by its session: its state does not say.
            let Some((mark, line)) = watched::read(&thread.live, |live| {
                (!thread.reviewer && is_thread(&live.info))
                    .then(|| (Into::<Mark>::into(live), line(live, &here)))
            }) else {
                continue;
            };
            let news = match session.saw(&thread.id) {
                Some(before) => before != mark,
                // One never seen that has yet to do anything says nothing.
                None => mark.turns > 0 || mark.stand != Stand::Working,
            };
            seen.push((thread.id.clone(), mark));

            if news {
                lines.push(line);
            }
        }

        News {
            text: (!lines.is_empty()).then(|| format!("{HEAD}\n{}", lines.join("\n"))),
            seen,
        }
    }
}

/// The line for the thread `live`, on the machine agents call `here`.
fn line(live: &Live, here: &str) -> String {
    let info = &live.info;
    let id = live.agent_session.as_ref().unwrap_or(&info.id);
    let id = full_id(id.as_ref(), here);
    let name = info.title.as_deref().unwrap_or(&info.agent);
    let stand = Stand::from(live);
    let text = match &info.question {
        Some(question) if stand == Stand::WaitingOnUser => one_line(&question.text),
        _ => one_line(&info.last_message),
    };
    let stand = stand.as_ref();

    if text.is_empty() {
        format!("- agent \"{name}\" {id}, {stand}")
    } else {
        format!("- agent \"{name}\" {id}, {stand}: {text}")
    }
}

/// `text` on one line, cut short.
fn one_line(text: &str) -> String {
    cut(&text.split_whitespace().collect::<Vec<_>>().join(" "), MAX_TEXT)
}
