//! What coding-agent sessions on nodes tell their chats: a turn ended, the
//! agent asks the user something, a followed session waits. Each goes to the chat
//! as a turn of its own once the chat is free, the way a finished research
//! reports back, and a question also goes to the
//! user as an approval card whose answer goes back to the agent.
//!
//! One task, made on first use, runs every delivery, a bounded number at a
//! time. It holds the app weakly, since the app owns it.

use super::Follower;
use crate::{
    app::{App, Draws, Message},
    protocol::{
        Approval, ApprovalField, DeviceKind,
        node::agent::{OptionKind, Question, Request},
    },
    store::CreateRun,
    user::User,
};
use flashmind::types::GateAnswer;
use std::{
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard, PoisonError, Weak},
    time::Duration,
};
use tokio::{
    sync::mpsc,
    task::JoinSet,
    time::{Instant, sleep_until},
};
use tokio_util::task::AbortOnDropHandle;
use uuid::Uuid;

/// Deliveries waiting to start; more are dropped.
const QUEUE: usize = 64;
/// Deliveries running at once.
const RUNNING: usize = 16;
/// How often a delivery to a busy chat tries again when no turn end woke
/// it, and how long it keeps trying.
const RETRY: Duration = Duration::from_secs(5);
const PATIENCE: Duration = Duration::from_secs(2 * 60 * 60);
/// The tool name an agent's permission card is saved under when the user
/// answers Always. It names no real tool, so Always only allows this kind
/// of card again, never a tool call.
const QUESTION_TOOL: &str = "node_agent_question";

/// A turn to start in a chat.
pub struct Post {
    pub user: Arc<User>,
    pub chat: Uuid,
    /// The turn's label, such as "Claude Code".
    pub label: String,
    pub text: String,
}

/// A permission question to put to the user as a card. The answer goes
/// back through `answer`, as the agent's session takes it; the card goes
/// away when `gone` ends first, as when the question was answered another
/// way.
pub struct Ask {
    pub follower: Follower,
    pub question: Question,
    /// The project task the session works on, shown on the card.
    pub task: Option<String>,
    pub gone: Pending,
    pub answer: Box<dyn FnOnce(Request) -> Pending + Send>,
}

pub type Pending = Pin<Box<dyn Future<Output = ()> + Send>>;

enum Job {
    Post(Post),
    Ask(Ask),
}

#[derive(Default)]
pub struct Deliveries(Mutex<Option<Runner>>);

struct Runner {
    jobs: mpsc::Sender<Job>,
    _task: AbortOnDropHandle<()>,
}

impl Deliveries {
    fn lock(&self) -> MutexGuard<'_, Option<Runner>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn post(&self, app: &Arc<App>, post: Post) {
        self.send(app, Job::Post(post));
    }

    pub fn ask(&self, app: &Arc<App>, ask: Ask) {
        self.send(app, Job::Ask(ask));
    }

    fn send(&self, app: &Arc<App>, job: Job) {
        let mut runner = self.lock();
        let runner = runner.get_or_insert_with(|| {
            let (jobs, queue) = mpsc::channel(QUEUE);
            let task = tokio::spawn(run(Arc::downgrade(app), queue));
            Runner {
                jobs,
                _task: AbortOnDropHandle::new(task),
            }
        });
        if runner.jobs.try_send(job).is_err() {
            log::warn!("nodes: dropped an agent session report, too many waiting");
        }
    }
}

async fn run(app: Weak<App>, mut queue: mpsc::Receiver<Job>) {
    let mut running = JoinSet::new();
    loop {
        tokio::select! {
            job = queue.recv(), if running.len() < RUNNING => {
                let Some(job) = job else { return };
                running.spawn(do_job(app.clone(), job));
            }
            Some(done) = running.join_next() => {
                if let Err(e) = done {
                    log::warn!("nodes: a delivery failed: {e}");
                }
            }
        }
    }
}

async fn do_job(app: Weak<App>, job: Job) {
    match job {
        Job::Post(post) => deliver(app, post).await,
        Job::Ask(ask) => put(app, ask).await,
    }
}

/// Starts a turn for `post` once its chat is free.
async fn deliver(app: Weak<App>, post: Post) {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let Some(app) = app.upgrade() else { return };
        // Registered before trying, so a turn ending in between still wakes it.
        let ended = app.subagents.turn_ended.notified();
        tokio::pin!(ended);
        ended.as_mut().enable();
        let message = message(post.chat, &post.label, post.text.clone());
        match app.submit(&post.user, message).await {
            Ok(CreateRun::Conflict) if Instant::now() < deadline => {}
            Ok(CreateRun::Conflict) => {
                log::warn!("nodes: chat {} stayed busy; report dropped", post.chat);
                return;
            }
            Ok(_) => return,
            Err(e) => {
                log::warn!("nodes: could not report to chat {}: {e:#}", post.chat);
                return;
            }
        }
        // The app is held for at most one wait.
        tokio::select! {
            _ = ended => {}
            _ = sleep_until(Instant::now() + RETRY) => {}
        }
    }
}

/// Shows the user the card for `ask` and hands the agent their answer.
async fn put(app: Weak<App>, ask: Ask) {
    let Some(strong) = app.upgrade() else { return };
    let Follower {
        user,
        chat,
        node,
        agent,
        ..
    } = &ask.follower;
    let answered = strong.subagents.approvals.ask(
        user.id(),
        *chat,
        QUESTION_TOOL,
        approval(node, agent, ask.task.as_deref(), &ask.question),
    );
    // The app is not held while the user thinks.
    drop(strong);
    let answer = tokio::select! {
        answer = answered => answer,
        // Dropping the card's future withdraws it.
        () = ask.gone => return,
    };
    let Some(request) = reply(&ask.question, &answer) else {
        log::info!("nodes: the agent's question had no option for the answer");
        return;
    };
    (ask.answer)(request).await;
}

/// The card for an agent's permission question.
fn approval(node: &str, agent: &str, task: Option<&str>, question: &Question) -> Approval {
    let mut fields = Vec::new();
    if let Some(task) = task {
        fields.push(ApprovalField {
            label: "Task".into(),
            value: task.to_owned(),
        });
    }
    fields.extend([
        ApprovalField {
            label: "Machine".into(),
            value: node.to_owned(),
        },
        ApprovalField {
            label: "Asks".into(),
            value: question.text.clone(),
        },
    ]);
    if let Some(detail) = &question.detail {
        fields.push(ApprovalField {
            label: "Detail".into(),
            value: detail.clone(),
        });
    }
    Approval {
        app: agent_name(agent).to_owned(),
        action: question.text.clone(),
        fields,
    }
}

pub(crate) fn agent_name(agent: &str) -> &str {
    match agent {
        "claude" => "Claude Code",
        "codex" => "Codex",
        "gemini" => "Gemini CLI",
        "pi" => "pi",
        "flsh" => "flsh",
        "opencode" => "OpenCode",
        other => other,
    }
}

/// The answer the agent gets for the card's answer: its allow or reject
/// option. `None` when it offers neither.
fn reply(question: &Question, answer: &GateAnswer) -> Option<Request> {
    let options = &question.options;
    let find = |kinds: &[OptionKind]| {
        kinds
            .iter()
            .find_map(|kind| options.iter().find(|option| option.kind == *kind))
    };
    let option = match answer {
        GateAnswer::Allow => find(&[OptionKind::AllowOnce, OptionKind::AllowAlways]),
        GateAnswer::Deny(_) => find(&[OptionKind::RejectOnce, OptionKind::RejectAlways]),
    }?;
    Some(Request::Answer {
        question: question.id,
        reply: option.id.clone(),
    })
}

/// A turn in `chat` that runs in the background and reaches the user like
/// a routine.
fn message(chat: Uuid, label: &str, text: String) -> Message {
    Message {
        session_id: chat,
        request_id: Uuid::new_v4(),
        message: text,
        cron_id: None,
        scheduled: Some(label.to_owned()),
        always_sent: true,
        woken: None,
        images: Vec::new(),
        files: Vec::new(),
        answer: None,
        model: None,
        thinking: None,
        origin: DeviceKind::Phone {},
        platform: None,
        draws: Draws::default(),
        finished_research: true,
        voice: None,
        spoken: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::node::agent::AnswerOption;

    fn question() -> Question {
        let option = |id: &str, kind| AnswerOption {
            id: id.into(),
            name: id.into(),
            kind,
        };
        Question {
            id: 3,
            text: "Run cargo test".into(),
            detail: Some("cargo test -p core".into()),
            options: vec![
                option("always", OptionKind::AllowAlways),
                option("once", OptionKind::AllowOnce),
                option("no", OptionKind::RejectOnce),
            ],
        }
    }

    #[test]
    fn a_card_answer_picks_the_agents_option() {
        let question = question();
        assert_eq!(
            reply(&question, &GateAnswer::Allow),
            Some(Request::Answer {
                question: 3,
                reply: "once".into()
            })
        );
        assert_eq!(
            reply(&question, &GateAnswer::Deny("no".into())),
            Some(Request::Answer {
                question: 3,
                reply: "no".into()
            })
        );
        let only_allow = Question {
            options: question.options[..2].to_vec(),
            ..question
        };
        assert_eq!(reply(&only_allow, &GateAnswer::Deny("no".into())), None);
    }

    #[test]
    fn the_card_names_the_agent_machine_and_command() {
        let card = approval("build-box", "codex", None, &question());
        assert_eq!(card.app, "Codex");
        assert_eq!(card.action, "Run cargo test");
        let values: Vec<&str> = card.fields.iter().map(|f| f.value.as_str()).collect();
        assert_eq!(
            values,
            ["build-box", "Run cargo test", "cargo test -p core"]
        );
        let card = approval("build-box", "codex", Some("fix-it: Fix it"), &question());
        assert_eq!(card.fields[0].label, "Task");
        assert_eq!(card.fields[0].value, "fix-it: Fix it");
    }
}
