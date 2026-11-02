//! Types shared by the project modules.

use crate::{
    protocol::{ProjectTaskState, ProjectWorkerState},
    store::Scope,
};
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

/// Longest owner or repository part of a [`RepoName`].
const MAX_REPO_PART: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RepoNameError {
    #[error("a repository is named owner/repo")]
    Shape,
    #[error(
        "each side of owner/repo is 1 to 100 letters, digits, '.', '_' or '-', and not '.' or '..'"
    )]
    Part,
}

/// A repository as `owner/repo`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct RepoName {
    name: String,
    /// Where the `/` is.
    slash: usize,
}

impl RepoName {
    pub(crate) fn parse(text: &str) -> Result<Self, RepoNameError> {
        let (owner, repo) = text.split_once('/').ok_or(RepoNameError::Shape)?;
        if repo.contains('/') {
            return Err(RepoNameError::Shape);
        }
        let valid = |part: &str| {
            (1..=MAX_REPO_PART).contains(&part.len())
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        };
        if !valid(owner) || !valid(repo) {
            return Err(RepoNameError::Part);
        }
        Ok(Self {
            name: text.to_owned(),
            slash: owner.len(),
        })
    }

    /// The repository part, without its owner.
    pub(crate) fn repo(&self) -> &str {
        &self.name[self.slash + 1..]
    }

    /// The memory scope the project's decisions and conventions go in.
    pub(crate) fn memory_scope(&self) -> anyhow::Result<Scope> {
        Scope::project(self.repo())
    }
}

impl fmt::Display for RepoName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum PathError {
    #[error("the path is not absolute")]
    Relative,
    #[error("the path has an empty, '.' or '..' part")]
    Segment,
    #[error("the path ends in '/'")]
    TrailingSlash,
}

/// An absolute path on a machine, with no `.`, `..` or empty part and no
/// trailing `/`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct MachinePath(String);

impl MachinePath {
    pub(crate) fn parse(text: &str) -> Result<Self, PathError> {
        let rest = text.strip_prefix('/').ok_or(PathError::Relative)?;
        if rest.ends_with('/') || rest.is_empty() {
            return Err(PathError::TrailingSlash);
        }
        if rest.split('/').any(|part| matches!(part, "" | "." | "..")) {
            return Err(PathError::Segment);
        }
        Ok(Self(text.to_owned()))
    }

    /// `rel` under this path. Empty, `.` and `..` parts of `rel` are
    /// dropped, so the result stays inside.
    pub(crate) fn join(&self, rel: &str) -> MachinePath {
        let mut joined = self.0.clone();
        for part in rel
            .split('/')
            .filter(|part| !matches!(*part, "" | "." | ".."))
        {
            joined.push('/');
            joined.push_str(part);
        }
        MachinePath(joined)
    }

    /// Whether `other` is this path or under it.
    pub(crate) fn contains(&self, other: &MachinePath) -> bool {
        other
            .0
            .strip_prefix(&self.0)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MachinePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Longest slug made from a title.
const MAX_SLUG: usize = 40;

/// A task's short name in its project, used in its branch and folders:
/// lowercase ASCII letters, digits and `-`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct TaskSlug(String);

impl TaskSlug {
    pub(crate) fn from_title(title: &str) -> TaskSlug {
        let mut slug = String::new();
        for c in title.chars() {
            if c.is_ascii_alphanumeric() {
                slug.push(c.to_ascii_lowercase());
            } else if !slug.is_empty() && !slug.ends_with('-') {
                slug.push('-');
            }
        }
        // Only ASCII is pushed, so any byte index is a char boundary.
        slug.truncate(MAX_SLUG);
        let slug = slug.trim_matches('-');
        if slug.is_empty() {
            TaskSlug("task".into())
        } else {
            TaskSlug(slug.into())
        }
    }

    /// A slug as stored, if it is one.
    pub(crate) fn parse(text: &str) -> Option<TaskSlug> {
        let valid = !text.is_empty()
            && !text.starts_with('-')
            && !text.ends_with('-')
            && text
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        valid.then(|| TaskSlug(text.into()))
    }

    /// `slug-n`, for a slug already taken.
    pub(crate) fn suffixed(&self, n: u32) -> TaskSlug {
        TaskSlug(format!("{}-{n}", self.0))
    }

    pub(crate) fn branch(&self) -> String {
        format!("beep/{}", self.0)
    }

    /// The task's worktree in `checkout`.
    pub(crate) fn worktree(&self, checkout: &MachinePath) -> MachinePath {
        checkout.join(&format!(".worktrees/{}", self.0))
    }

    /// Where inputs land in the task's `worktree`.
    pub(crate) fn incoming(&self, worktree: &MachinePath) -> MachinePath {
        worktree.join(&format!(".beep/incoming/{}", self.0))
    }

    /// Where the task's outputs go in the `home` checkout.
    pub(crate) fn outputs(&self, home: &MachinePath) -> MachinePath {
        home.join(&format!(".beep/outputs/{}", self.0))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TaskSlug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

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

/// Whether a project's repository is public, which decides what its issue
/// and PR text may say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr, EnumString)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum Visibility {
    Public,
    Private,
}

/// A Forgejo server's hostname.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ForgejoHost(String);

impl ForgejoHost {
    /// A hostname of letters, digits, `.` and `-`, if `text` is one.
    pub(crate) fn parse(text: &str) -> Option<ForgejoHost> {
        let valid = !text.is_empty()
            && !text.starts_with(['.', '-'])
            && !text.ends_with(['.', '-'])
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'));
        valid.then(|| ForgejoHost(text.to_ascii_lowercase()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// Where a project's issues or PRs live.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum IssueHost {
    Github,
    Forgejo(ForgejoHost),
}

impl IssueHost {
    /// `github` or `forgejo:<host>`, as the store keeps it.
    pub(crate) fn stored(&self) -> String {
        match self {
            Self::Github => "github".into(),
            Self::Forgejo(host) => format!("forgejo:{}", host.as_str()),
        }
    }

    pub(crate) fn from_stored(text: &str) -> Option<IssueHost> {
        if text == "github" {
            return Some(Self::Github);
        }
        let host = text.strip_prefix("forgejo:")?;
        ForgejoHost::parse(host).map(Self::Forgejo)
    }
}

/// Which hosts a project files its issues and opens its PRs on. `None` keeps
/// that part in Beep only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Roles {
    pub(crate) issues: Option<IssueHost>,
    pub(crate) prs: Option<IssueHost>,
}

/// A task's issue on its host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IssueRef {
    pub(crate) number: u64,
    pub(crate) url: String,
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
    /// Whether it puts each action to Beep before it runs it. flsh and
    /// OpenCode do not, so their workers run commands nobody judged.
    pub(crate) fn asks_permission(self) -> bool {
        super::policy::agent_asks(self.into())
    }

    /// The protocol name a node serves this agent on.
    pub(crate) fn protocol(self) -> String {
        agent::protocol(self.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_agent_names_round_trip() {
        assert_eq!("claude".parse(), Ok(WorkerAgent::Claude));
        assert_eq!(WorkerAgent::Codex.to_string(), "codex");
        assert_eq!(WorkerAgent::Codex.protocol(), agent::protocol("codex"));
    }

    #[test]
    fn every_agent_a_node_knows_can_work() {
        for name in ["claude", "codex", "gemini", "pi", "flsh", "opencode"] {
            let agent: WorkerAgent = name.parse().expect("a worker agent");
            assert_eq!(agent.to_string(), name);
        }
        assert_eq!(
            WorkerAgent::VARIANTS,
            ["claude", "codex", "gemini", "pi", "flsh", "opencode"]
        );
        assert_eq!(
            serde_json::to_value(WorkerAgent::OpenCode).ok(),
            Some(serde_json::json!("opencode"))
        );
        assert_eq!(WorkerAgent::OpenCode.protocol(), "opencode/1");
        // flsh and OpenCode run their tools without asking, so nothing can
        // judge them.
        assert!(!WorkerAgent::Flsh.asks_permission());
        assert!(!WorkerAgent::OpenCode.asks_permission());
        assert!(WorkerAgent::Claude.asks_permission());
        assert!(WorkerAgent::Pi.asks_permission());
    }

    #[test]
    fn repo_names_parse() {
        let name = RepoName::parse("acme/widget").expect("a valid name");
        assert_eq!(name.to_string(), "acme/widget");
        assert_eq!(name.repo(), "widget");
        assert_eq!(
            name.memory_scope().expect("a valid scope"),
            Scope::project("widget").expect("a valid scope")
        );
        for bad in [
            "acme", "a/b/c", "../x", "acme/", "ac me/x", "./x", "acme/..",
        ] {
            assert!(RepoName::parse(bad).is_err(), "{bad} should not parse");
        }
    }

    #[test]
    fn slugs_from_titles() {
        assert_eq!(
            TaskSlug::from_title("Fix the Android build!").as_str(),
            "fix-the-android-build"
        );
        let long = TaskSlug::from_title(&format!("{}-{}", "a".repeat(39), "b".repeat(20)));
        assert_eq!(long.as_str(), "a".repeat(39));
        assert_eq!(TaskSlug::from_title("!!!").as_str(), "task");
        let slug = TaskSlug::from_title("Fix it");
        assert_eq!(slug.suffixed(2).as_str(), "fix-it-2");
        assert_eq!(slug.branch(), "beep/fix-it");
        let checkout = MachinePath::parse("/srv/project").expect("a valid path");
        let worktree = slug.worktree(&checkout);
        assert_eq!(worktree.as_str(), "/srv/project/.worktrees/fix-it");
        assert_eq!(
            slug.incoming(&worktree).as_str(),
            "/srv/project/.worktrees/fix-it/.beep/incoming/fix-it"
        );
        assert_eq!(
            slug.outputs(&checkout).as_str(),
            "/srv/project/.beep/outputs/fix-it"
        );
    }

    #[test]
    fn paths_join_and_contain() {
        let project = MachinePath::parse("/srv/project").expect("a valid path");
        let inside = MachinePath::parse("/srv/project/.worktrees/x").expect("a valid path");
        let beside = MachinePath::parse("/srv/project-old").expect("a valid path");
        assert!(project.contains(&inside));
        assert!(project.contains(&project));
        assert!(!project.contains(&beside));
        assert!(!inside.contains(&project));
        assert_eq!(project.join(".worktrees/x"), inside);
        // A join cannot climb out of the path.
        assert_eq!(project.join("../x").as_str(), "/srv/project/x");
        for bad in ["/srv/../etc", "srv/project", "/srv/project/", "", "/srv//x"] {
            assert!(MachinePath::parse(bad).is_err(), "{bad} should not parse");
        }
    }

    #[test]
    fn issue_hosts_round_trip() {
        let host = ForgejoHost::parse("git.example.com").expect("a valid host");
        let forgejo = IssueHost::Forgejo(host);
        assert_eq!(forgejo.stored(), "forgejo:git.example.com");
        assert_eq!(IssueHost::from_stored(&forgejo.stored()), Some(forgejo));
        assert_eq!(IssueHost::from_stored("github"), Some(IssueHost::Github));
        assert_eq!(IssueHost::from_stored("forgejo:"), None);
        assert_eq!(IssueHost::from_stored("gitlab"), None);
    }

    #[test]
    fn states_map_to_the_wire() {
        for state in [
            TaskState::Planned,
            TaskState::Running,
            TaskState::Blocked,
            TaskState::Review,
            TaskState::Done,
            TaskState::Dropped,
        ] {
            assert_eq!(TaskState::from(ProjectTaskState::from(state)), state);
            let name: &str = state.into();
            assert_eq!(name.parse(), Ok(state));
        }
        assert!(TaskState::Blocked.is_open());
        assert!(!TaskState::Done.is_open());
        assert!(TaskState::Review.wants_checks());
        assert!(!TaskState::Planned.wants_checks());
        assert!(WorkerState::Waiting.is_live());
        assert!(!WorkerState::Failed.is_live());
        assert_eq!(
            ProjectWorkerState::from(WorkerState::Ended),
            ProjectWorkerState::Ended {}
        );
    }
}
