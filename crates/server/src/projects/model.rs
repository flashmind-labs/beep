//! Types shared by the project modules.

use crate::{
    protocol::{ProjectTaskState, ProjectWorkerState},
    store::Scope,
};
pub(crate) use beep_projects::model::*;
use beep_proto::node::agent;
use serde::{Deserialize, Serialize};
use std::{fmt, time::Duration};
use strum::{Display, EnumString, IntoStaticStr, VariantNames};
use uuid::Uuid;

/// Most live workers in one project.
pub(crate) const MAX_PROJECT_WORKERS: i64 = 3;
/// Most live workers on one machine, across projects.
pub(crate) const MAX_MACHINE_WORKERS: i64 = 2;
/// Most live workers one user has.
pub(crate) const MAX_USER_WORKERS: i64 = 6;
/// How often the agent checks CI and reviews while a task runs or is in review.
pub(crate) const CHECK_EVERY: Duration = Duration::from_secs(10 * 60);
/// How long a worker may sit idle after its last turn before it is closed.
pub(crate) const IDLE_CLOSE: Duration = Duration::from_secs(30 * 60);

/// The memory scope the project's decisions and conventions go in.
pub(crate) fn memory_scope(name: &RepoName) -> anyhow::Result<Scope> {
    Scope::project(name.repo())
}

/// A uuid that names one kind of row.
macro_rules! row_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub(crate) struct $name(pub(crate) Uuid);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

row_id!(
    /// A project's row id.
    ProjectId
);
row_id!(
    /// A project task's row id.
    TaskId
);
row_id!(
    /// A project worker's row id.
    WorkerId
);

/// Where a project task stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr, EnumString)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum TaskState {
    Planned,
    Running,
    Blocked,
    Review,
    Done,
    Dropped,
}

impl TaskState {
    /// Not done or dropped.
    pub(crate) fn is_open(self) -> bool {
        !matches!(self, Self::Done | Self::Dropped)
    }

    /// Whether the check timer looks at its CI and reviews.
    pub(crate) fn wants_checks(self) -> bool {
        matches!(self, Self::Running | Self::Review)
    }
}

impl From<TaskState> for ProjectTaskState {
    fn from(state: TaskState) -> Self {
        match state {
            TaskState::Planned => Self::Planned {},
            TaskState::Running => Self::Running {},
            TaskState::Blocked => Self::Blocked {},
            TaskState::Review => Self::Review {},
            TaskState::Done => Self::Done {},
            TaskState::Dropped => Self::Dropped {},
        }
    }
}

impl From<ProjectTaskState> for TaskState {
    fn from(state: ProjectTaskState) -> Self {
        match state {
            ProjectTaskState::Planned {} => Self::Planned,
            ProjectTaskState::Running {} => Self::Running,
            ProjectTaskState::Blocked {} => Self::Blocked,
            ProjectTaskState::Review {} => Self::Review,
            ProjectTaskState::Done {} => Self::Done,
            ProjectTaskState::Dropped {} => Self::Dropped,
        }
    }
}

/// What a project worker is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr, EnumString)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum WorkerState {
    Starting,
    Running,
    Waiting,
    Ended,
    Failed,
}

impl WorkerState {
    /// The states that hold a slot and the folder lock.
    pub(crate) const LIVE: [WorkerState; 3] = [Self::Starting, Self::Running, Self::Waiting];

    pub(crate) fn is_live(self) -> bool {
        Self::LIVE.contains(&self)
    }
}

impl From<WorkerState> for ProjectWorkerState {
    fn from(state: WorkerState) -> Self {
        match state {
            WorkerState::Starting => Self::Starting {},
            WorkerState::Running => Self::Running {},
            WorkerState::Waiting => Self::Waiting {},
            WorkerState::Ended => Self::Ended {},
            WorkerState::Failed => Self::Failed {},
        }
    }
}

/// How the agent asked a task to start, kept until a slot frees.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct StartRequest {
    /// The machine the user named.
    #[serde(default)]
    pub(crate) machine: Option<String>,
    #[serde(default)]
    pub(crate) agent: Option<WorkerAgent>,
    #[serde(default)]
    pub(crate) persona: Option<String>,
    #[serde(default)]
    pub(crate) inputs: Vec<InputFile>,
    /// The branch the worktree starts from instead of the default, such as a reviewed branch.
    #[serde(default)]
    pub(crate) base: Option<String>,
}

/// A file to copy into the worktree before the worker starts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct InputFile {
    /// The machine it is on, when the agent named one.
    #[serde(default)]
    pub(crate) machine: Option<String>,
    pub(crate) path: String,
}

/// The coding agent a worker machine runs for a project.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    Display,
    EnumString,
    IntoStaticStr,
    VariantNames,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub(crate) enum WorkerAgent {
    Claude,
    Codex,
    Gemini,
    Pi,
    Flsh,
    OpenCode,
}

impl WorkerAgent {
    /// The protocol name a node serves this agent on.
    pub(crate) fn protocol(self) -> String {
        agent::protocol(self.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_start_kept_before_bases_reads_from_the_default_branch() {
        let kept: StartRequest =
            serde_json::from_str(r#"{"machine":null,"agent":"claude","persona":null,"inputs":[]}"#)
                .expect("a start kept before bases");
        assert_eq!(kept.base, None);
        assert_eq!(kept.agent, Some(WorkerAgent::Claude));
    }

    #[test]
    fn a_repo_has_a_memory_scope_by_its_name() {
        let name = RepoName::parse("acme/widget").expect("a valid name");
        assert_eq!(
            memory_scope(&name).expect("a valid scope"),
            Scope::project("widget").expect("a valid scope")
        );
    }
}
