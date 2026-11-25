//! What the view layer follows. Whoever changes state a view shows rings it with a [`Change`];
//! `event_views` follows it and draws. It depends on no feature, so any module may ring it.

use beep_proto::{
    HostedRuns, LabeledProjectLists,
    node::events::{self, Topic},
};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
/// What a window follows: a topic, on one machine or on all.
pub struct Interest {
    pub machine: Option<String>,
    pub topic: Topic,
}

impl Interest {
    /// Whether this is an interest in a controller view that `topic` names.
    pub(crate) fn wants_view(&self, topic: &Topic) -> bool {
        self.machine.is_none() && self.topic.matches(topic)
    }

    /// Whether this is an interest in what `machine` pushes on `topic`.
    pub(crate) fn wants_node(&self, machine: &str, topic: &Topic) -> bool {
        self.machine.as_ref().is_none_or(|own| own == machine)
            && (self.topic.matches(topic)
                || matches!(
                    (topic.name(), self.topic.name()),
                    ("agents", "agent") | ("processes", "process-output")
                ))
    }
}

/// What the windows want, as `Events` last published it. `Views` keeps this copy to decide what
/// to draw, so a change never asks `Events`.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Wants {
    /// The windows that have a reader attached, hidden ones too.
    windows: HashSet<String>,
    /// What the shown windows follow, the standing interests aside.
    shown: Vec<Interest>,
    /// What the shown and briefly hidden windows follow, the standing interests aside.
    held: Vec<Interest>,
}

impl Wants {
    pub(crate) fn new(
        windows: HashSet<String>,
        mut shown: Vec<Interest>,
        mut held: Vec<Interest>,
    ) -> Self {
        shown.sort();
        shown.dedup();
        held.sort();
        held.dedup();

        Self {
            windows,
            shown,
            held,
        }
    }

    /// Whether a shown window wants the controller view `topic`, so it is drawn now.
    pub(crate) fn controller_wanted(&self, topic: &Topic) -> bool {
        self.shown.iter().any(|interest| interest.wants_view(topic))
    }

    /// Whether a shown or briefly hidden window wants the controller view `topic`, so a drawn
    /// view is kept for it.
    pub(crate) fn controller_held(&self, topic: &Topic) -> bool {
        self.held.iter().any(|interest| interest.wants_view(topic))
    }

    /// Whether a shown or briefly hidden window follows what `machine` pushes on `topic`.
    pub(crate) fn node_wanted(&self, machine: EndpointId, topic: &Topic) -> bool {
        let machine = machine.to_string();
        self.held
            .iter()
            .any(|interest| interest.wants_node(&machine, topic))
    }

    pub(crate) fn window_labels(&self) -> &HashSet<String> {
        &self.windows
    }
}

#[derive(Clone, Serialize)]
/// What a window is sent for a topic.
pub struct Update {
    pub(crate) machine_id: String,
    pub(crate) revision: u64,
    pub(crate) topic: Topic,
    pub(crate) query: Option<u64>,
    pub(crate) initial: bool,
    pub(crate) data: Option<serde_json::Value>,
    /// Local order of retained updates, for priming a window with only what it has not seen.
    pub(crate) seq: u64,
}

/// The most paths an update names, as the node does.
const MAX_CHANGED: usize = 256;

/// The paths the node says changed in the folder, from the listing a `files` update carries;
/// empty when it does not say.
pub(crate) fn changed_paths(update: &Update) -> Vec<String> {
    update
        .data
        .as_ref()
        .and_then(|data| data.pointer("/Sessions/Tree/changed"))
        .and_then(|paths| serde_json::from_value(paths.clone()).ok())
        .unwrap_or_default()
}

/// Gives `update` the paths of `earlier` too; unknown when either is.
pub(crate) fn join_changed(earlier: &Update, update: &mut Update) {
    let (before, now) = (changed_paths(earlier), changed_paths(update));
    let mut joined: Vec<String> = before.into_iter().chain(now.iter().cloned()).collect();
    joined.sort();
    joined.dedup();

    if changed_paths(earlier).is_empty() || now.is_empty() || joined.len() > MAX_CHANGED {
        joined.clear();
    }

    if let Some(slot) = update
        .data
        .as_mut()
        .and_then(|data| data.pointer_mut("/Sessions/Tree/changed"))
    {
        *slot = json!(joined);
    }
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
    /// A node pushed a folder's `files` topic; what is followed there is read again.
    FilesHeard { machine: EndpointId, update: Update },
    /// The windows' subscriptions changed: what no window follows any more is forgotten. What
    /// the windows follow is read when this is applied, not when it was rung: terminal output
    /// may have kept it waiting while a window followed a folder again.
    FilesKept,
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
    if app.state::<ViewSignal>().0.send(change).is_err() {
        log::warn!("a change was rung after the views stopped following");
    }
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
