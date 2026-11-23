//! A coding-agent notice, and the key in its `session_id` that says which
//! session it is about, so a click opens the session and Allow or Deny
//! answers its question.

use crate::SessionId;
use crate::node::agent::{Alert, AlertKind, StopReason};
use std::{fmt, path::Path, str::FromStr};

/// A notification about a coding-agent session.
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub title: String,
    pub body: String,
    /// An `AgentNoticeKey`, or empty for a notice about no session.
    pub session_id: String,
}

/// The prefix of every key, which no chat or group id has.
const PREFIX: &str = "agent:";

/// A coding-agent session a notice is about, and the question it asks.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentNoticeKey {
    /// The node's key.
    pub machine_id: String,
    /// The agent, such as `claude`.
    pub agent: String,
    pub session: SessionId,
    /// The question waiting for an answer; `None` for a turn that ended.
    pub question: Option<u32>,
    /// The machine's name, such as `build-box`.
    pub machine_name: String,
}

impl AgentNoticeKey {
    /// Whether this key is about `session` of `agent` on `machine_id`.
    pub fn is_session(&self, machine_id: &str, agent: &str, session: &SessionId) -> bool {
        self.machine_id == machine_id && self.agent == agent && self.session == *session
    }
}

/// `agent:<question>:<machine id>:<agent>:<session>:<machine name>`, the
/// question empty when there is none. The name goes last, so it may hold
/// a colon.
impl fmt::Display for AgentNoticeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(PREFIX)?;

        if let Some(question) = self.question {
            write!(f, "{question}")?;
        }

        write!(
            f,
            ":{}:{}:{}:{}",
            self.machine_id, self.agent, self.session, self.machine_name
        )
    }
}

/// A `session_id` that is not an agent key.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not a coding-agent notice key")]
pub struct NotAgentKey;

impl FromStr for AgentNoticeKey {
    type Err = NotAgentKey;

    fn from_str(key: &str) -> Result<Self, Self::Err> {
        let rest = key.strip_prefix(PREFIX).ok_or(NotAgentKey)?;
        let mut parts = rest.splitn(5, ':');
        let mut next = || parts.next().ok_or(NotAgentKey);
        let question = match next()? {
            "" => None,
            id => Some(id.parse().map_err(|_| NotAgentKey)?),
        };
        let machine_id = next()?.to_owned();
        let agent = next()?.to_owned();
        let session = next()?.to_owned();
        let machine_name = next()?.to_owned();

        if machine_id.is_empty() || agent.is_empty() || session.is_empty() {
            return Err(NotAgentKey);
        }

        Ok(Self {
            machine_id,
            agent,
            session: session.into(),
            question,
            machine_name,
        })
    }
}

/// The most characters of a notice body; the session has the rest.
const MAX_BODY: usize = 240;

impl Alert {
    /// The key a computer reads this alert's notice by.
    pub fn key(&self, machine_id: &str, machine_name: &str) -> AgentNoticeKey {
        let question = match &self.kind {
            AlertKind::Asks { question } => Some(question.id),
            _ => None,
        };
        AgentNoticeKey {
            machine_id: machine_id.to_owned(),
            agent: self.agent.clone(),
            session: self.session.clone(),
            question,
            machine_name: machine_name.to_owned(),
        }
    }

    /// "org/web: Fix the login", or only the session's title when the
    /// session is in no project. The row's icon says which agent it is.
    pub fn notice_title(&self, project: Option<&str>) -> String {
        let about = self.about();

        match project {
            Some(project) => format!("{project}: {about}"),
            None => about,
        }
    }

    /// What happened, on one short line: the agent's reply, its question,
    /// or why it stopped.
    pub fn notice_body(&self) -> String {
        let said = match &self.kind {
            AlertKind::Asks { question } => match &question.detail {
                Some(detail) => format!("{} ({detail})", question.text),
                None => question.text.clone(),
            },
            AlertKind::TurnEnded {
                stop_reason,
                last_message,
                error,
            } => match stop_reason {
                StopReason::EndTurn if last_message.trim().is_empty() => "Finished".to_owned(),
                StopReason::EndTurn => last_message.clone(),
                StopReason::Failed => match error {
                    Some(error) => format!("Failed: {error}"),
                    None => "Failed".to_owned(),
                },
                StopReason::Other => "Stopped".to_owned(),
                _ => format!("Stopped: {stop_reason}"),
            },
            AlertKind::Failed { reason } => format!("Failed: {reason}"),
        };
        clip(&said)
    }

    /// The session's title, or else its folder's name, or else its id.
    fn about(&self) -> String {
        if let Some(title) = self.title.as_deref().filter(|t| !t.trim().is_empty()) {
            return title.trim().to_owned();
        }

        Path::new(&self.cwd)
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or(self.session.as_ref())
            .to_owned()
    }
}

/// The name of the coding agent `agent`, as people say it.
pub fn agent_name(agent: &str) -> &str {
    match agent {
        "claude" => "Claude Code",
        "codex" => "Codex",
        "agy" => "Antigravity",
        "pi" => "pi",
        "flsh" => "flsh",
        "opencode" => "OpenCode",
        "hermes" => "Hermes",
        other => other,
    }
}

/// `text` as markdown, cut to `MAX_BODY` characters; apps render it.
fn clip(text: &str) -> String {
    let text = text.trim();

    match text.char_indices().nth(MAX_BODY) {
        Some((end, _)) => format!("{}...", text[..end].trim_end()),
        None => text.to_owned(),
    }
}

#[cfg(test)]
#[path = "../tests/agents_notice.rs"]
mod tests;
