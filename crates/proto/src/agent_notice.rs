//! What a coding-agent notice is about, as a computer reads it from
//! `Notice::session_id`.
//!
//! Phones get coding-agent notices with an empty `session_id`, which they
//! open as Beep itself. A computer gets the same notice with this key in
//! `session_id` instead, so a click opens the session and Allow or Deny
//! answers its question. The `Notice` keeps its shape; a computer from
//! before the key reads it as no chat and opens Home.

use std::{fmt, str::FromStr};

/// The prefix of every key, which no chat or group id has.
const PREFIX: &str = "agent:";

/// A coding-agent session a notice is about, and the question it asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentNoticeKey {
    /// The node's key.
    pub machine_id: String,
    /// The agent, such as `claude`.
    pub agent: String,
    pub session: String,
    /// The question waiting for an answer; `None` for a turn that ended.
    pub question: Option<u32>,
    /// The machine's name, such as `build-box`.
    pub machine_name: String,
}

impl AgentNoticeKey {
    /// Whether this key is about `session` of `agent` on `machine_id`.
    pub fn is_session(&self, machine_id: &str, agent: &str, session: &str) -> bool {
        self.machine_id == machine_id && self.agent == agent && self.session == session
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
            session,
            question,
            machine_name,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(question: Option<u32>) -> AgentNoticeKey {
        AgentNoticeKey {
            machine_id: "ab12".into(),
            agent: "claude".into(),
            session: "a3f9c1".into(),
            question,
            machine_name: "build-box: lab".into(),
        }
    }

    #[test]
    fn a_key_reads_back_as_written() {
        for question in [None, Some(7)] {
            let written = key(question).to_string();
            assert!(written.starts_with("agent:"), "{written}");
            assert_eq!(written.parse::<AgentNoticeKey>(), Ok(key(question)));
        }
        assert_eq!(
            key(Some(7)).to_string(),
            "agent:7:ab12:claude:a3f9c1:build-box: lab"
        );
    }

    #[test]
    fn chat_and_group_ids_are_not_keys() {
        for id in [
            "",
            "6f1c1d36-3f0e-4c55-9d6b-2b8f3c1d9e10",
            "g1",
            "agent:",
            "agent:x:ab12:claude:a3f9c1:box",
            "agent::ab12::a3f9c1:box",
            "agent::ab12:claude",
        ] {
            assert_eq!(id.parse::<AgentNoticeKey>(), Err(NotAgentKey), "{id}");
        }
    }
}
