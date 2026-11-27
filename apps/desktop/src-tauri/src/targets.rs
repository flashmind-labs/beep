//! The views the window can be asked to open, as the bar, the tray, a notice or another window
//! name them. Plain data: `center::open_on` carries them out.

use beep_proto::{AgentNoticeKey, SessionId};
use serde::{Deserialize, Serialize};

/// A view of the window that the bar, the tray or another window can hand
/// over to.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CenterTarget {
    Home,
    Settings,
    Instructions,
    /// A coding agent's session, as a Running now row names it.
    Agent(Box<AgentTarget>),
    Providers,
    /// The Projects page on `project`, or every project with `None`, as a
    /// project window hands over another project. With `run`, on its Actions
    /// tile with that run open, as a build's notification asks.
    Projects {
        project: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run: Option<RunAt>,
    },
    Terminals,
    /// Machines in Settings, as a beep-node update's notification asks.
    Machines,
    /// The Report a problem sheet, over the page on screen.
    Report,
    /// A pull request or issue on its git host, in the browser.
    Link(String),
}

/// A CI run of a project: its id on `host`, as the Actions tile keys runs.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub(crate) struct RunAt {
    pub(crate) id: u64,
    pub(crate) host: String,
}

/// A coding agent's session to open, shaped as the window's Running now
/// row (`RunningAgent`) so it opens like one.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub(crate) struct AgentTarget {
    kind: String,
    machine_id: String,
    machine_name: String,
    id: SessionId,
    agent: String,
    title: Option<String>,
    cwd: String,
    status: String,
    doing: Option<String>,
    started_at: u64,
    last_activity: u64,
    external: bool,
    /// A temporary reviewer: it opens beside the diff it reviewed.
    #[serde(default)]
    reviewer: bool,
}

impl AgentTarget {
    /// Agent `agent`'s session `session` on machine `machine_id`, which
    /// waits on a question, boxed as `CenterTarget::Agent` holds it.
    pub(crate) fn asking(
        machine_id: &str,
        machine_name: &str,
        agent: &str,
        session: &SessionId,
        title: Option<&str>,
    ) -> Box<Self> {
        Box::new(Self {
            kind: "agent".into(),
            machine_id: machine_id.to_owned(),
            machine_name: machine_name.to_owned(),
            id: session.clone(),
            agent: agent.to_owned(),
            title: title.map(str::to_owned),
            cwd: String::new(),
            status: "asking".into(),
            doing: None,
            started_at: 0,
            last_activity: 0,
            external: false,
            reviewer: false,
        })
    }

    /// The same session, a temporary reviewer.
    pub(crate) fn reviewing(mut self: Box<Self>) -> Box<Self> {
        self.reviewer = true;
        self
    }

    /// The session a coding-agent notice is about: asking while its
    /// question waits, idle once a turn ended.
    pub(crate) fn notified(key: &AgentNoticeKey) -> Box<Self> {
        let mut target = Self::asking(
            &key.machine_id,
            &key.machine_name,
            &key.agent,
            &key.session,
            None,
        );
        if key.question.is_none() {
            target.status = "idle".into();
        }
        target
    }
}

#[cfg(test)]
#[path = "tests/targets.rs"]
mod tests;
