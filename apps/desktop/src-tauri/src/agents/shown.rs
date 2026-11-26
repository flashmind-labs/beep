//! The coding-agent session each webview shows, in `AgentsShown`, so
//! notices for what is on screen stay quiet.

use beep_proto::AgentNoticeKey;
use beep_proto::SessionId;
use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};

/// How long after a webview last read an agent session it still counts as
/// shown there. Live subscriptions refresh this on each update or heartbeat;
/// older nodes are read at most 10 seconds apart.
const AGENT_SHOWN_FOR: Duration = Duration::from_secs(12);

/// The coding-agent session each webview's session view last read, and
/// when, by webview label.
#[derive(Debug, Default)]
pub(crate) struct AgentsShown(Mutex<HashMap<String, ShownAgent>>);

#[derive(Debug)]
struct ShownAgent {
    machine_id: String,
    agent: String,
    session: SessionId,
    at: Instant,
}

impl AgentsShown {
    fn views(&self) -> MutexGuard<'_, HashMap<String, ShownAgent>> {
        crate::store::lock(&self.0)
    }

    /// Webview `label` read `session` of `agent` on `machine_id` at `at`.
    pub(crate) fn read(
        &self,
        label: &str,
        machine_id: &str,
        agent: &str,
        session: &SessionId,
        at: Instant,
    ) {
        let shown = ShownAgent {
            machine_id: machine_id.to_owned(),
            agent: agent.to_owned(),
            session: session.clone(),
            at,
        };
        self.views().insert(label.to_owned(), shown);
    }

    /// Forgets webview `label`'s session view, when it closes.
    pub(crate) fn clear(&self, label: &str) {
        self.views().remove(label);
    }

    /// Whether one of `labels`, the webviews on screen, read the session
    /// `key` is about lately, as of `now`.
    pub(crate) fn shows(&self, labels: &[&str], key: &AgentNoticeKey, now: Instant) -> bool {
        let views = self.views();
        labels.iter().any(|label| {
            views.get(*label).is_some_and(|shown| {
                key.is_session(&shown.machine_id, &shown.agent, &shown.session)
                    && now.saturating_duration_since(shown.at) < AGENT_SHOWN_FOR
            })
        })
    }
}

#[cfg(test)]
#[path = "../tests/agents_shown.rs"]
mod tests;
