//! What the view layer follows. Whoever changes state a view shows rings it with a [`Change`];
//! `event_views` follows it and draws. It depends on no feature, so any module may ring it.

use beep_proto::{
    HostedRuns, LabeledProjectLists,
    node::events::{self, Topic},
};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
/// What a window follows: a topic, on one machine or on all.
pub struct Interest {
    pub machine: Option<String>,
    pub topic: Topic,
}

/// What changed, in the order it was rung.
pub(crate) enum Change {
    /// The state behind a topic changed; `*` is all of it.
    Topic(Topic),
    /// A sync change; `moving` when it only reports files in transit.
    Sync { moving: bool },
    /// A machine's sessions, one session or the moves changed.
    Sessions(Topic),
    /// The user changed a session on this desktop.
    Edited,
    /// A window showed again.
    Resumed,
    /// A source failed to read.
    Failed { topic: Topic, reason: String },
    /// What the page of a followed project hears.
    Project(LiveRow),
    /// A node answered on a team topic; `None` when it refused.
    Team {
        machine: EndpointId,
        topic: Topic,
        value: Option<events::Value>,
    },
    /// A node from before the topics of one screen said its terminals or tasks changed.
    TerminalsChanged { machine: EndpointId, topic: Topic },
    /// A node pushed the screen of one of its terminals or tasks.
    OutputPushed {
        machine: EndpointId,
        topic: Topic,
        value: events::Value,
    },
    /// The mesh's members changed: a team's record or a contact may have.
    Mesh,
}

/// Takes the changes to `event_views::follow_changes`.
pub(crate) struct ViewSignal(mpsc::UnboundedSender<Change>);

impl ViewSignal {
    pub(crate) fn new() -> (Self, mpsc::UnboundedReceiver<Change>) {
        let (sender, changes) = mpsc::unbounded_channel();
        (Self(sender), changes)
    }
}

/// Rings `change`.
pub(crate) fn ring(app: &AppHandle, change: Change) {
    // The receiver lives as long as the app.
    let _ = app.state::<ViewSignal>().0.send(change);
}

/// What the page hears of one project.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct LiveRow {
    pub(crate) project_id: Uuid,
    /// None while it is not known.
    pub(crate) live: Option<bool>,
    pub(crate) polling_why: Option<String>,
    pub(crate) lists: bool,
    pub(crate) runs: bool,
    /// What the read that caused this row found, so the draw does not read again.
    #[serde(skip)]
    pub(crate) lists_read: Option<LabeledProjectLists>,
    #[serde(skip)]
    pub(crate) runs_read: Option<HostedRuns>,
    pub(crate) views: Vec<View>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct View {
    command: &'static str,
    args: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl View {
    pub(crate) fn new<T: Serialize>(command: &'static str, args: Value, value: T) -> Option<Self> {
        match serde_json::to_value(value) {
            Ok(value) => Some(Self {
                command,
                args,
                value: Some(value),
                error: None,
            }),
            Err(error) => {
                log::debug!("view snapshot {command} could not be encoded: {error}");
                Some(Self::failed(command, args, &error.to_string()))
            }
        }
    }

    pub(crate) fn failed(command: &'static str, args: Value, reason: &str) -> Self {
        Self {
            command,
            args,
            value: None,
            error: Some(reason.to_owned()),
        }
    }
}

/// Whether a sync snapshot shows files in transit on that machine.
pub(crate) fn moving(value: &beep_proto::node::events::Value) -> bool {
    use beep_proto::node::{events::Value, sync::Reply};

    matches!(value, Value::Sync(reply) if matches!(reply.as_ref(),
        Reply::Status { projects } if projects.iter().any(|project| project.progress.is_some())))
}
