//! Reports from a node session to its chat: a finished turn or unwaited question starts a turn there.

use super::{AgentLinks, LinkOrder, Watched, order};
use crate::{
    app::App,
    nodes::{Ask, Follower, Post, deliver::agent_name},
    projects::Handled,
    protocol::node::agent::{Event, Question},
    tools::machines::agent_call,
};
use iroh::EndpointId;
use std::{
    fmt::Write,
    sync::{
        Arc, Weak,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::sync::{mpsc, watch};

/// The longest last message a report quotes.
const MAX_QUOTED: usize = 4000;

/// What a session did that its chat hears about.
#[derive(Debug, Clone)]
pub(crate) enum News {
    TurnDone {
        turn: u32,
        last_message: String,
    },
    Question(Question),
    /// The session ended. Only project workers act on it.
    Ended {
        reason: String,
    },
}

impl News {
    pub(super) fn of(event: &Event) -> Option<Self> {
        match event {
            Event::TurnDone {
                turn, last_message, ..
            } => Some(Self::TurnDone {
                turn: *turn,
                last_message: last_message.clone(),
            }),
            Event::Question { question } => Some(Self::Question(question.clone())),
            Event::Ended { reason } => Some(Self::Ended {
                reason: reason.clone(),
            }),
            _ => None,
        }
    }
}

/// Tells a session's chat what it did. Held by the session's link task.
pub(super) struct Reporter {
    pub(super) app: Weak<App>,
    pub(super) links: Weak<AgentLinks>,
    /// The node the session runs on.
    pub(super) machine: EndpointId,
    pub(super) session: String,
    pub(super) agent: String,
    pub(super) waiting: Arc<AtomicUsize>,
    pub(super) orders: mpsc::Sender<LinkOrder>,
    pub(super) state: watch::Receiver<Watched>,
}

impl Reporter {
    pub(super) fn tell(&self, news: News) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        // A call waiting for the session gets this as its result.
        let nobody_waits = self.waiting.load(Ordering::SeqCst) == 0;
        // A project's worker reports to its project instead.
        let handled =
            app.projects
                .on_news(&app, &self.machine, &self.session, &news, !nobody_waits);
        if handled == Handled::Yes {
            return;
        }
        let Some(owner) = self
            .links
            .upgrade()
            .and_then(|links| links.owner(&self.session))
        else {
            return;
        };
        let deliveries = &app.nodes.deliveries;
        match news {
            News::TurnDone { turn, last_message } => {
                if nobody_waits {
                    let text =
                        turn_report(&owner.node, &self.agent, &self.session, turn, &last_message);
                    deliveries.post(&app, self.post(owner, text));
                }
            }
            News::Question(question) => {
                if nobody_waits {
                    let text = question_report(&owner.node, &self.agent, &self.session, &question);
                    deliveries.post(&app, self.post(owner.clone(), text));
                }
                deliveries.ask(&app, self.ask(owner, question));
            }
            News::Ended { .. } => {}
        }
    }

    fn post(&self, follower: Follower, text: String) -> Post {
        Post {
            label: agent_name(&self.agent).to_owned(),
            user: follower.user,
            chat: follower.chat,
            text,
        }
    }

    /// The card for `question`; it goes away once answered elsewhere or the session goes.
    fn ask(&self, follower: Follower, question: Question) -> Ask {
        ask_card(
            &self.state,
            &self.orders,
            self.session.clone(),
            follower,
            question,
            None,
        )
    }
}

/// The card for `question` of `session`; it goes away once answered elsewhere or the session goes.
pub(super) fn ask_card(
    state: &watch::Receiver<Watched>,
    orders: &mpsc::Sender<LinkOrder>,
    session: String,
    follower: Follower,
    question: Question,
    task: Option<String>,
) -> Ask {
    let id = question.id;
    let mut state = state.clone();
    let orders = orders.clone();
    Ask {
        follower,
        question,
        task,
        gone: Box::pin(async move {
            let open = |now: &Watched| now.info.question.as_ref().map(|q| q.id) == Some(id);
            // An error means the session's link is gone, and so is the question.
            let _ = state.wait_for(|now| !open(now)).await;
        }),
        answer: Box::new(move |request| {
            Box::pin(async move {
                if let Err(e) = order(&orders, request).await {
                    log::info!("nodes: answer not taken by session {session}: {e:#}");
                }
            })
        }),
    }
}

fn turn_report(node: &str, agent: &str, session: &str, turn: u32, last_message: &str) -> String {
    let name = agent_name(agent);
    let mut text = format!(
        "The {name} session {session} on {node} that you started in this chat finished \
         turn {turn}."
    );
    let last = last_message.trim();
    if !last.is_empty() {
        let _ = write!(text, " Its last message:\n{}", quote(last));
    }
    let _ = write!(
        text,
        "\n\nTell the user in a few sentences what it did. {} gives it more to do and {} \
         reads more.",
        agent_call(node, agent, "prompt", session),
        agent_call(node, agent, "transcript", session)
    );
    text
}

fn question_report(node: &str, agent: &str, session: &str, question: &Question) -> String {
    let name = agent_name(agent);
    let mut text = format!(
        "The {name} session {session} on {node} that you started in this chat asks: {}",
        question.text
    );
    if let Some(detail) = &question.detail {
        let _ = write!(text, " ({detail})");
    }
    let options: Vec<&str> = question.options.iter().map(|o| o.id.as_str()).collect();
    let _ = write!(
        text,
        "\nThe user has a card to answer it. When it is clearly within what the user asked, \
         you can answer instead with {} and the option as `prompt`; its options are {}. Leave \
         a risky or destructive one, such as a force push, to the user. Tell the user briefly \
         what it waits for.",
        agent_call(node, agent, "answer", session),
        options.join(", ")
    );
    text
}

fn quote(text: &str) -> String {
    match text.char_indices().nth(MAX_QUOTED) {
        Some((end, _)) => format!("{}...", &text[..end]),
        None => text.to_owned(),
    }
}
