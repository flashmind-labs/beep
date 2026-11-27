//! What an agent reads of one child session (`ChildView`).

use crate::text::cut;
use beep_proto::SessionId;
use beep_proto::node::agent::{SessionInfo, Status};
use serde::{Deserialize, Serialize};

/// The longest title or message the agent reads back.
const MAX_TEXT: usize = 2000;

/// What an agent reads of one child.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ChildView {
    /// The id the other tools take: `<session>@<machine>`.
    pub(crate) id: String,
    /// The id its machine lists it under, which the agent's own id
    /// replaces after a restart: how an app finds its row.
    #[serde(default)]
    pub(crate) key: SessionId,
    pub(crate) harness: String,
    pub(crate) machine: String,
    /// `working`, `waiting_on_user`, `idle` (its turn is done, its last
    /// message is the answer), `ended` or `unreachable`.
    pub(crate) state: String,
    pub(crate) title: Option<String>,
    pub(crate) last_message: String,
    pub(crate) files_changed: usize,
    /// What it waits for the user to allow or answer. Only the user can.
    pub(crate) question: Option<String>,
    pub(crate) turns: u32,
    pub(crate) tokens: u64,
    pub(crate) cost_usd: Option<f64>,
    pub(crate) error: Option<String>,
    /// Said when `wait` ran out of time or the child's turn is done.
    #[serde(default)]
    pub(crate) note: Option<String>,
    /// The project whose checkout holds its folder, in a list of every agent.
    #[serde(default)]
    pub(crate) project: Option<String>,
    /// The branch of the checkout it works in, as its node last read it.
    #[serde(default)]
    pub(crate) branch: Option<String>,
    /// Said by a node that tells its parent's of every settle (`family::notes`),
    /// so a wait on it needs no recheck. An older node does not say it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) notes_settles: bool,
    /// The team issue its first prompt names. Appended: an older node
    /// does not say it.
    #[serde(default)]
    pub(crate) issue: Option<u64>,
}

impl ChildView {
    /// The view of a session as its own node reports it; `machine` and the
    /// id's machine part are set by whoever names the machine.
    pub(crate) fn of(info: &SessionInfo, id: &SessionId) -> Self {
        let state = match info.state.status {
            Status::Ended => "ended",
            Status::Asking => "waiting_on_user",
            Status::Idle if info.state.subagents == 0 => "idle",
            _ => "working",
        };

        Self {
            id: id.to_string(),
            key: info.id.clone(),
            harness: info.agent.clone(),
            machine: String::new(),
            state: state.to_owned(),
            title: info.title.clone(),
            last_message: cut(&info.last_message, MAX_TEXT),
            files_changed: info.state.files_touched.len(),
            question: info
                .question
                .as_ref()
                .map(|q| cut(&q.text, MAX_TEXT)),
            turns: info.turns,
            tokens: info.tokens,
            cost_usd: info.cost_usd,
            error: info.state.error.clone(),
            note: None,
            project: None,
            branch: info.place.as_ref().and_then(|place| place.branch.clone()),
            notes_settles: false,
            issue: info.issue,
        }
    }

    pub(crate) fn named(mut self, machine: &str) -> Self {
        self.machine = machine.to_owned();
        self.id = full_id(&self.id, machine);
        self
    }

    pub(crate) fn settled(&self) -> bool {
        self.state != "working"
    }
}

/// The id the other tools take for the child `id` on `machine`:
/// `<session>@<machine>`, or the bare id without a machine name.
pub(crate) fn full_id(id: &str, machine: &str) -> String {
    if machine.is_empty() || id.contains('@') {
        id.to_owned()
    } else {
        format!("{id}@{machine}")
    }
}
