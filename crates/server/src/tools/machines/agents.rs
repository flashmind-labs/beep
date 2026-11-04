//! Coding-agent sessions on a node, for `code_agent`.

use super::agent_call;
use crate::{
    account::user::User,
    app::App,
    nodes::{Follower, Online, StartOn, Watched},
    protocol::node::agent::{
        EntryKind, ExternalSession, LiveState, LiveStatus, PlanEntry, PlanStatus, Request,
        SessionInfo, Status, Steer,
    },
};
use chrono::Utc;
use std::{fmt::Write, sync::Arc, time::Duration};
use uuid::Uuid;

pub(crate) const DEFAULT_WAIT_SECS: u64 = 90;
pub(super) const MAX_WAIT_SECS: u64 = 30 * 60;
/// How long `cancel` and `close` wait for the agent to stop.
const STOP_WAIT: Duration = Duration::from_secs(10);
pub(crate) const DEFAULT_LAST: u32 = 20;
/// The longest transcript entry shown.
const MAX_ENTRY_SHOWN: usize = 2000;

/// The only agent whose sessions started outside Beep a node finds.
pub(super) const EXTERNAL_AGENT: &str = "claude";
/// The most outside sessions `sessions --all` lists.
const EXTERNAL_LISTED: u32 = 15;

/// The chat a command runs in, for following sessions.
pub(crate) struct Asker<'a> {
    pub(crate) app: &'a Arc<App>,
    pub(crate) user: &'a Arc<User>,
    pub(crate) chat: Uuid,
    /// The node's key, as the store keeps it.
    pub(crate) endpoint: &'a str,
}

impl Asker<'_> {
    /// This chat, as a session reporting to it knows it.
    fn follower(&self, node: &str, agent: &str) -> Follower {
        Follower {
            user: self.user.clone(),
            chat: self.chat,
            node: node.to_owned(),
            agent: agent.to_owned(),
        }
    }
}

/// What `code_agent` asks of a session or of the agent.
#[derive(Debug, PartialEq)]
pub(crate) enum Order {
    Start {
        cwd: Option<String>,
        prompt: String,
        wait: Duration,
    },
    /// One session, or with `id` `None` this agent's sessions.
    Status {
        id: Option<String>,
        wait: Option<Duration>,
    },
    Prompt {
        id: String,
        text: String,
        wait: Duration,
    },
    Answer {
        id: String,
        text: String,
        wait: Duration,
    },
    Cancel {
        id: String,
    },
    Close {
        id: String,
    },
    Transcript {
        id: String,
        last: u32,
    },
    /// This agent's sessions, and with `all` those started outside Beep.
    Sessions {
        all: bool,
    },
    Follow {
        id: String,
    },
    Unfollow {
        id: String,
    },
    Takeover {
        id: String,
        prompt: Option<String>,
        wait: Duration,
    },
    /// How a session started outside Beep does now, from its hooks.
    Live {
        id: String,
    },
    /// Queues a message or a stop for a session started outside Beep.
    Steer {
        id: String,
        steer: Steer,
    },
}

/// Runs one order; the text is the result, the error a failure.
pub(crate) async fn run(
    online: &Online,
    node: &str,
    agent: &str,
    order: Order,
    asker: &Asker<'_>,
) -> Result<String, String> {
    let call = Call {
        online,
        node,
        agent,
    };
    match order {
        Order::Start { cwd, prompt, wait } => {
            let link = online
                .start_agent(agent, cwd, prompt, StartOn::default())
                .await
                .map_err(|e| format!("Could not start {agent}: {e:#}"))?;
            let now = link.now();
            online.report_to(&now.info.id, asker.follower(node, agent));
            let mark = now.mark();
            Ok(call.report(&link.wait(mark, wait).await, Some(wait)))
        }
        Order::Status { id: None, .. } => call.list().await,
        Order::Status { id: Some(id), wait } => {
            let link = call.attach(&id).await?;
            let now = link.now();
            Ok(match wait {
                Some(wait) if !now.settled() => {
                    call.report(&link.wait(now.mark(), wait).await, Some(wait))
                }
                _ => call.report(&now, None),
            })
        }
        Order::Prompt { id, text, wait } => call.send(&id, text, wait, false, asker).await,
        Order::Answer { id, text, wait } => call.send(&id, text, wait, true, asker).await,
        Order::Cancel { id } => call.stop(&id, true).await,
        Order::Close { id } => call.stop(&id, false).await,
        Order::Transcript { id, last } => {
            let (entries, earlier) = online
                .transcript(&id, last, false)
                .await
                .map_err(|e| format!("Could not read the transcript: {e:#}"))?;
            let mut out = String::new();
            if earlier > 0 {
                let _ = writeln!(out, "({earlier} earlier entries)");
            }
            for entry in entries {
                let text = cut(&entry.text, MAX_ENTRY_SHOWN);
                let _ = writeln!(out, "[{}] {text}", entry.kind.as_str());
                if entry.kind == EntryKind::Agent {
                    out.push('\n');
                }
            }
            if out.is_empty() {
                out.push_str("The transcript is empty.");
            }
            Ok(out)
        }
        Order::Sessions { all } => {
            let mut out = call.list().await?;
            if all && agent == EXTERNAL_AGENT {
                out.push_str("\n\n");
                out.push_str(&call.external().await?);
            }
            Ok(out)
        }
        Order::Follow { .. }
        | Order::Unfollow { .. }
        | Order::Takeover { .. }
        | Order::Live { .. }
        | Order::Steer { .. }
            if agent != EXTERNAL_AGENT =>
        {
            Err(format!(
                "Only Claude Code sessions started outside Beep can be followed, checked live, \
                 steered or taken over, not {agent}."
            ))
        }
        Order::Live { id } => online
            .live_session(&id, None)
            .await
            .map(|status| live_report(&status))
            .map_err(|e| format!("Could not check {id}: {e:#}")),
        Order::Steer { id, steer } => {
            let queued = match &steer {
                Steer::Stop {} => "Queued a stop: its next tool call is denied.",
                _ => {
                    "Queued: Claude reads it after the current tool call, or instead of ending \
                      its turn."
                }
            };
            let status = online
                .live_session(&id, Some(steer))
                .await
                .map_err(|e| format!("Could not steer {id}: {e:#}"))?;
            Ok(format!("{queued}\n{}", live_report(&status)))
        }
        Order::Follow { id } => call.follow(&id, asker).await,
        Order::Unfollow { id } => {
            let nodes = &asker.app.nodes;
            Ok(if nodes.follows.unfollow(asker.app, asker.endpoint, &id) {
                format!("Stopped following {id} on {node}.")
            } else {
                format!("{id} was not followed on {node}.")
            })
        }
        Order::Takeover { id, prompt, wait } => {
            let prompted = prompt.is_some();
            let link = online
                .take_over(agent, &id, prompt)
                .await
                .map_err(|e| format!("Could not take over {id}: {e:#}"))?;
            // It runs here now; the node session reports on it.
            asker
                .app
                .nodes
                .follows
                .unfollow(asker.app, asker.endpoint, &id);
            let now = link.now();
            online.report_to(&now.info.id, asker.follower(node, agent));
            let shown = if prompted {
                call.report(&link.wait(now.mark(), wait).await, Some(wait))
            } else {
                call.report(&now, None)
            };
            Ok(format!(
                "Took over {id}: it now runs on {node} as session {}, so steer it with {}.\n{shown}",
                now.info.id,
                agent_call(node, agent, "prompt", &now.info.id)
            ))
        }
    }
}

/// A session started outside Beep as its hooks report it: one short line to say aloud,
/// then the details.
fn live_report(status: &LiveStatus) -> String {
    let session = &status.session;
    let mut out = match status.state {
        LiveState::Working => {
            let mut line = "Working".to_owned();
            if let Some(tool) = &status.tool {
                let _ = write!(line, ", running {}", tool.name);
                if !tool.detail.is_empty() {
                    let _ = write!(line, " ({})", cut(&tool.detail, 80));
                }
                let _ = write!(line, " for {}", span(tool.secs));
            }
            line.push('.');
            line
        }
        LiveState::Asking => format!(
            "Waiting for the user in the terminal: {}.",
            status.waiting_for.trim_end_matches('.')
        ),
        LiveState::Idle => {
            "Idle at its prompt, so it cannot be steered; only the user can prompt it there."
                .to_owned()
        }
        LiveState::Ended => "It ended.".to_owned(),
        _ => format!(
            "{}; no hook reported on it since the node started, so it cannot be seen live or \
             steered. That needs `beep-node hooks install` and a restart of the session.",
            crate::apps::permissions::capitalize(session.presence.as_str())
        ),
    };
    let done = status
        .todos
        .iter()
        .filter(|todo| todo.status == PlanStatus::Completed)
        .count();
    if !status.todos.is_empty() {
        let _ = write!(out, " {done} of {} todos done.", status.todos.len());
    }
    if status.error.is_some() {
        out.push_str(" A tool call failed.");
    }
    out.push('\n');
    let _ = write!(out, "Session {} in {}", session.id, session.cwd);
    if let Some(title) = &session.title {
        let _ = write!(out, ", \"{title}\"");
    }
    out.push('\n');
    match &status.queued {
        Some(Steer::Message { text }) => {
            let _ = writeln!(out, "Waiting for its next hook: \"{text}\"");
        }
        Some(_) => out.push_str("Waiting for its next hook: a stop.\n"),
        None => {}
    }
    if let Some(error) = &status.error {
        let _ = writeln!(out, "Last error: {error}");
    }
    if let Some(now) = status
        .todos
        .iter()
        .find(|todo| todo.status == PlanStatus::InProgress)
    {
        let _ = writeln!(out, "Todo now: {}", now.content);
    }
    let left: Vec<&PlanEntry> = status
        .todos
        .iter()
        .filter(|todo| todo.status == PlanStatus::Pending)
        .collect();
    if !left.is_empty() {
        let left: Vec<&str> = left.iter().map(|todo| todo.content.as_str()).collect();
        let _ = writeln!(out, "Todos left: {}", left.join("; "));
    }
    if !status.files.is_empty() {
        let _ = writeln!(out, "Files changed: {}", status.files.join(", "));
    }
    if !session.last_message.is_empty() {
        let _ = writeln!(
            out,
            "Last message: {}",
            cut(&session.last_message, MAX_ENTRY_SHOWN)
        );
    }
    out.trim_end().to_owned()
}

/// "40s" or "3 min".
fn span(secs: u64) -> String {
    match secs {
        0..60 => format!("{secs}s"),
        _ => format!("{} min", secs / 60),
    }
}

/// Sessions started outside Beep for the model, newest first.
fn external_list(node: &str, sessions: &[ExternalSession], now: u64) -> String {
    if sessions.is_empty() {
        return format!("No Claude Code sessions started outside Beep on {node}.");
    }
    let mut out = format!("Claude Code sessions started outside Beep on {node}, newest first:\n");
    for session in sessions {
        out.push_str(&external_line(session, now));
    }
    out.trim_end().to_owned()
}

fn external_line(session: &ExternalSession, now: u64) -> String {
    let mut out = format!("- {} in {}: {}", session.id, session.cwd, session.presence);
    if let Some(pid) = session.pid {
        let _ = write!(out, " (pid {pid})");
    }
    let _ = write!(
        out,
        ", last active {}",
        ago(now.saturating_sub(session.last_activity))
    );
    if let Some(title) = &session.title {
        let _ = write!(out, ", \"{title}\"");
    }
    if let Some(node_session) = &session.taken_over {
        let _ = write!(out, ", runs here as session {node_session}");
    }
    out.push('\n');
    if !session.last_prompt.is_empty() {
        let _ = writeln!(out, "  Last prompt: {}", session.last_prompt);
    }
    if !session.last_message.is_empty() {
        let _ = writeln!(out, "  Last message: {}", session.last_message);
    }
    out
}

fn ago(secs: u64) -> String {
    match secs / 60 {
        0 => "just now".into(),
        minutes @ 1..60 => format!("{minutes} min ago"),
        minutes @ 60..1440 => format!("{} h ago", minutes / 60),
        minutes => format!("{} days ago", minutes / 1440),
    }
}

fn unix_now() -> u64 {
    u64::try_from(Utc::now().timestamp()).unwrap_or(0)
}

/// Every agent session on the node.
pub(super) async fn sessions(online: &Online, node: &str) -> Result<String, String> {
    let sessions = online
        .agent_sessions()
        .await
        .map_err(|e| format!("Could not list the sessions: {e:#}"))?;
    Ok(list(node, &sessions))
}

fn list(node: &str, sessions: &[SessionInfo]) -> String {
    if sessions.is_empty() {
        return format!("No agent sessions on {node}.");
    }
    let mut out = String::new();
    for info in sessions {
        let _ = write!(
            out,
            "- {} {} in {}: {}, {} turns",
            info.id, info.agent, info.cwd, info.state.status, info.turns
        );
        if let Some(title) = &info.title {
            let _ = write!(out, ", \"{title}\"");
        }
        out.push('\n');
    }
    out
}

struct Call<'a> {
    online: &'a Online,
    node: &'a str,
    agent: &'a str,
}

impl Call<'_> {
    async fn attach(&self, id: &str) -> Result<std::sync::Arc<crate::nodes::AgentLink>, String> {
        self.online
            .agent_session(self.agent, id)
            .await
            .map_err(|e| format!("Could not reach session {id}: {e:#}"))
    }

    async fn list(&self) -> Result<String, String> {
        let mut sessions = self
            .online
            .agent_sessions()
            .await
            .map_err(|e| format!("Could not list the sessions: {e:#}"))?;
        sessions.retain(|info| info.agent == self.agent);
        Ok(list(self.node, &sessions))
    }

    async fn external(&self) -> Result<String, String> {
        let sessions = self
            .online
            .external_sessions(self.agent, EXTERNAL_LISTED, None)
            .await
            .map_err(|e| format!("Could not list the sessions started outside Beep: {e:#}"))?;
        Ok(external_list(self.node, &sessions, unix_now()))
    }

    async fn follow(&self, id: &str, asker: &Asker<'_>) -> Result<String, String> {
        let found = self
            .online
            .external_sessions(self.agent, 1, Some(id.to_owned()))
            .await
            .map_err(|e| format!("Could not look up {id}: {e:#}"))?;
        let Some(session) = found.first() else {
            return Err(format!(
                "No Claude Code session {id} on {}. code_agent with action `sessions` and \
                 `all: true` lists them.",
                self.node
            ));
        };
        if let Some(node_session) = &session.taken_over {
            return Err(format!(
                "{id} runs here as session {node_session}; {} checks it.",
                agent_call(self.node, self.agent, "status", node_session)
            ));
        }
        let follower = asker.follower(self.node, self.agent);
        asker
            .app
            .nodes
            .follows
            .follow(asker.app, asker.endpoint, id, follower)
            .map_err(|e| format!("Could not follow {id}: {e:#}"))?;
        let live = if session.presence.is_live() {
            "It is open in the user's terminal or IDE, so Beep can only follow it; only the \
             user can answer it there."
        } else {
            "Nothing runs it now; action `takeover` would resume it here."
        };
        Ok(format!(
            "Following {id} on {}. This chat hears when it finishes a turn; when it waits \
             for permission too if beep-node's hooks are installed there \
             (`beep-node hooks install`). {live}\n{}",
            self.node,
            external_line(session, unix_now()).trim_end()
        ))
    }

    /// Sends a prompt, or an answer to the open question, and waits.
    async fn send(
        &self,
        id: &str,
        text: String,
        wait: Duration,
        answering: bool,
        asker: &Asker<'_>,
    ) -> Result<String, String> {
        let link = self.attach(id).await?;
        let before = link.now();
        let request = if answering {
            let Some(question) = &before.info.question else {
                return Err(format!(
                    "Session {id} has no open question.\n{}",
                    self.report(&before, None)
                ));
            };
            Request::Answer {
                question: question.id,
                reply: text,
            }
        } else {
            Request::Prompt { text }
        };
        self.online
            .report_to(&before.info.id, asker.follower(self.node, self.agent));
        link.send(request)
            .await
            .map_err(|e| format!("The node did not take it: {e:#}"))?;
        Ok(self.report(&link.wait(before.mark(), wait).await, Some(wait)))
    }

    /// Cancels the turn, or with `cancel` false closes the session.
    async fn stop(&self, id: &str, cancel: bool) -> Result<String, String> {
        let link = self.attach(id).await?;
        let before = link.now();
        let request = if cancel {
            Request::Cancel {}
        } else {
            Request::Close {}
        };
        link.send(request)
            .await
            .map_err(|e| format!("The node did not take it: {e:#}"))?;
        let after = if before.settled() && cancel {
            before
        } else {
            link.wait(before.mark(), STOP_WAIT).await
        };
        Ok(self.report(&after, None))
    }

    fn report(&self, watched: &Watched, waited: Option<Duration>) -> String {
        describe(self.node, self.agent, watched, waited)
    }
}

/// A session's state for the model, with how long the call `waited` for the turn.
fn describe(node: &str, agent: &str, watched: &Watched, waited: Option<Duration>) -> String {
    let info = &watched.info;
    let state = &info.state;
    let id = &info.id;
    let status = match (&watched.ended, watched.lost) {
        (Some(reason), _) => format!("ended ({reason})"),
        (None, true) => "lost track of it; it may still run, check its status".to_owned(),
        (None, false) => state.status.to_string(),
    };
    let mut out = format!(
        "{agent} session {id} in {}: {status}, {} turns",
        info.cwd, info.turns
    );
    if state.queued > 0 {
        let _ = write!(out, ", {} prompts waiting", state.queued);
    }
    out.push('\n');
    if let Some(title) = &info.title {
        let _ = writeln!(out, "Title: {title}");
    }
    if state.status == Status::Working && !state.summary.is_empty() {
        let _ = writeln!(out, "Doing: {}", state.summary);
    }
    if !state.plan.is_empty() {
        out.push_str("Plan:\n");
        for entry in &state.plan {
            let mark = match entry.status {
                PlanStatus::Completed => "done",
                PlanStatus::InProgress => "now",
                _ => "todo",
            };
            let _ = writeln!(out, "  [{mark}] {}", entry.content);
        }
    }
    if !state.files_touched.is_empty() {
        let _ = writeln!(out, "Files changed: {}", state.files_touched.join(", "));
    }
    if let Some(error) = &state.error
        && watched.ended.as_ref() != Some(error)
    {
        let _ = writeln!(out, "Error: {error}");
    }
    if let Some(question) = &info.question {
        let _ = writeln!(out, "Question {}: {}", question.id, question.text);
        if let Some(detail) = &question.detail {
            let _ = writeln!(out, "  {detail}");
        }
        let options: Vec<String> = question
            .options
            .iter()
            .enumerate()
            .map(|(n, option)| format!("{}. {} ({})", n + 1, option.name, option.id))
            .collect();
        let _ = writeln!(out, "Options: {}", options.join(", "));
        let _ = writeln!(
            out,
            "Answer with {} and the option as `prompt`. Any other text declines and goes \
             to the agent as its next prompt. Answer only when it is clearly within what the \
             user asked; leave a risky or destructive one, such as a force push, to the user.",
            agent_call(node, agent, "answer", id)
        );
    }
    if info.turns > 0 && state.status != Status::Working && !info.last_message.is_empty() {
        let stop = watched
            .last_stop
            .map_or("ended".to_owned(), |stop| stop.to_string());
        let _ = writeln!(
            out,
            "Last turn {stop}. Its last message:\n{}",
            info.last_message
        );
    }
    if let Some(waited) = waited
        && matches!(state.status, Status::Working | Status::Starting)
        && watched.ended.is_none()
        && !watched.lost
    {
        let _ = writeln!(
            out,
            "Still working after {}s. Check later with {} and `wait_secs: 120`.",
            waited.as_secs(),
            agent_call(node, agent, "status", id)
        );
    }
    out.trim_end().to_owned()
}

fn cut(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{} [...]", &text[..end])
}
