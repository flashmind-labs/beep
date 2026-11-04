//! Projects: their branches, imports, issues, threads, tasks and workers.

mod ci;
mod git;
pub mod groups;
mod reviews;
mod sync;
mod workspace;

pub use ci::*;
pub use git::*;
pub use reviews::*;
pub use sync::*;
pub use workspace::*;

use crate::FoundAgent;
use std::time::SystemTime;
use uuid::Uuid;

wire_structs! {
    /// What `WatchProject` streams.
    pub struct ProjectChange {
        /// The host's events reach Beep, so the lists and runs are read again
        /// within seconds of a change. False while they are read every few
        /// minutes only.
        pub live: bool,
        /// Why the project is not live, for the user; `None` when it is, or
        /// while that is not known yet.
        pub polling_why: Option<String>,
        /// The pull request and issue lists were read again since the last
        /// reply.
        pub lists: bool,
        /// The workflow runs were read again since the last reply.
        pub runs: bool,
    }

    /// What `GetProjectLists` gives.
    pub struct ProjectLists {
        pub pull_requests: Vec<ProjectPullRequest>,
        /// Why the pull requests could not be read; the list is then the one
        /// read before, if any.
        pub prs_error: Option<String>,
        /// The open issues no task tracks, newest first.
        pub issues: Vec<ProjectIssue>,
        /// As `prs_error`, for the issues.
        pub issues_error: Option<String>,
        /// When the host was last read; `None` before the first read and for a
        /// project without a GitHub or Forgejo host.
        pub synced_at: Option<SystemTime>,
    }

    /// What `GetLabeledProjectLists` gives.
    pub struct LabeledProjectLists {
        pub lists: ProjectLists,
        /// The label names of each of `lists.pull_requests`, in its order.
        pub pr_labels: Vec<Vec<String>>,
    }
}

ffi_structs! {
    /// What `FindImportFork` found.
    pub struct ImportFork {
        /// `org/repo`, the repository asked about.
        pub project: String,
        /// The caller's fork, `login/repo`; `None` when it is not cloned.
        pub fork: Option<String>,
        /// The head branches of the caller's open pull requests from the fork.
        pub branches: Vec<String>,
    }

    /// Folders the apps list projects under, as `ListProjectGroups` gives
    /// them. A project is in a group when its folder on any machine is one of
    /// `paths` or inside one, both compared as `groups::project_group_path`
    /// gives them.
    pub struct ProjectGroup {
        pub id: Uuid,
        /// What the group is called: its first folder until renamed.
        pub name: String,
        /// The folders, such as `work`, oldest first.
        pub paths: Vec<String>,
    }

    /// A project's branches on one machine, as `ListProjectBranches` reads them.
    pub struct ProjectBranches {
        pub machine_id: String,
        pub machine_name: String,
        pub default_branch: String,
        pub branches: Vec<ProjectBranch>,
    }

    /// One branch of a project.
    pub struct ProjectBranch {
        /// Without `origin/`.
        pub name: String,
        /// The folder of the worktree that has it checked out, on the machine
        /// read.
        pub worktree: Option<String>,
        /// Commits it has that the default branch does not.
        pub ahead: u32,
        /// Commits the default branch has that it does not.
        pub behind: u32,
        /// Its last commit.
        pub head: String,
        pub subject: String,
        pub committed_at: SystemTime,
        /// It is a branch of the checkout, not only on `origin`.
        pub local: bool,
        /// It is on `origin`.
        pub pushed: bool,
    }

    /// A branch to give a worktree, and where.
    pub struct BranchPlace {
        pub branch: String,
        /// The machine; `None` for the one the project is imported on.
        pub machine_id: Option<String>,
    }

    /// What `PreviewImport` found for a repository.
    pub struct ImportPreview {
        /// `org/repo`.
        pub project: String,
        /// The checkouts found, the one the branches were read in first.
        pub checkouts: Vec<ImportCheckout>,
        /// Empty when no checkout was read.
        pub default_branch: String,
        pub branches: Vec<WorkBranch>,
        /// What could not be read, such as the pull requests; empty when all
        /// was.
        pub note: String,
    }

    /// A checkout of a repository on one of the caller's machines.
    pub struct ImportCheckout {
        pub machine_id: String,
        pub machine_name: String,
        pub folder: String,
        /// The remote that is the repository, such as `origin` or `upstream`.
        pub remote: String,
    }

    /// A branch with work, as `PreviewImport` lists it.
    pub struct WorkBranch {
        pub name: String,
        /// The caller's open pull request from it.
        pub pr: Option<u64>,
        pub pr_title: String,
        /// Commits ahead of the default branch; 0 when not read.
        pub ahead: u32,
        /// Its worktree's folder, on the first checkout's machine.
        pub worktree: Option<String>,
        /// It is in the checkout read, locally or on `origin`.
        pub in_checkout: bool,
        /// Its last commit; `None` when not read.
        pub committed_at: Option<SystemTime>,
    }

    /// One project as `ListProjectOverview` sums it up.
    pub struct ProjectOverview {
        pub project_id: Uuid,
        /// `org/repo`.
        pub name: String,
        /// How many of its tasks stand in each state, dropped ones left out.
        pub tasks: TaskCounts,
        /// Its workers that have not ended.
        pub workers: u32,
        /// The task that changed last, dropped ones left out.
        pub latest: Option<ProjectTask>,
    }

    /// How many of a project's tasks stand in each state.
    #[derive(Copy, Default, Eq)]
    pub struct TaskCounts {
        pub planned: u32,
        pub running: u32,
        pub blocked: u32,
        pub review: u32,
        pub done: u32,
    }

    /// A project imported onto one of the caller's machines.
    pub struct ImportedProject {
        /// `org/repo`.
        pub project: String,
        /// The repository's web page, such as `https://github.com/org/repo`.
        pub url: String,
        /// The `Machine::id` it is on.
        pub machine_id: String,
        pub machine_name: String,
        /// Its absolute folder on that machine.
        pub folder: String,
        /// This import cloned it, rather than finding it already there.
        pub cloned: bool,
        pub imported_at: SystemTime,
    }

    /// A git checkout the caller's coding agents worked in, as
    /// `ListFoundFolders` lists it.
    pub struct FoundProject {
        /// `org/repo` from its remote, else the remote as it is.
        pub project: String,
        /// Its `origin` remote, what `ImportProject::repo` takes.
        pub remote: String,
        /// Where it is, one folder per machine.
        pub places: Vec<FoundPlace>,
        /// Sessions per agent, on all of its machines, most first.
        pub agents: Vec<FoundAgent>,
        /// When a session there last changed or its checked-out commit was
        /// made, whichever is newer.
        pub last_activity: SystemTime,
    }

    /// One git host's repositories, as `ListHostRepos` lists them.
    pub struct HostRepos {
        /// `github.com`, or a Forgejo server's host, such as `git.example.com`.
        pub host: String,
        pub kind: GitHostKind,
        /// Newest update first.
        pub repos: Vec<HostRepo>,
        /// Why `repos` is empty, for the user; `None` when the host answered.
        pub error: Option<String>,
    }

    /// A repository on a git host, as `ListHostRepos` lists it.
    pub struct HostRepo {
        /// `org/repo`.
        pub name: String,
        /// Its web page, what `ImportProject::repo` takes.
        pub url: String,
        pub updated_at: SystemTime,
    }

    /// One machine's checkout of a `FoundProject`.
    pub struct FoundPlace {
        pub machine_id: String,
        pub machine_name: String,
        /// Its absolute folder on that machine.
        pub folder: String,
    }

    /// One of the caller's machines as a project's page lists it.
    pub struct ProjectMachine {
        pub machine_id: String,
        pub machine_name: String,
        pub online: bool,
        /// The project's absolute folder there; `None` without a checkout.
        pub folder: Option<String>,
        /// Beep is looking through it for a checkout now.
        pub checking: bool,
        /// When Beep last looked through it, since the project was imported.
        pub checked_at: Option<SystemTime>,
    }

    /// Where `CopyProject` put a project.
    pub struct ProjectCopy {
        /// The project's absolute folder on the machine.
        pub folder: String,
        /// The machine it comes from; `None` when it was there already.
        pub from: Option<String>,
    }

    /// An open issue on a project's issues host, as `ListProjectIssues` gives it.
    pub struct ProjectIssue {
        pub number: u64,
        pub title: String,
        /// Its label names.
        pub labels: Vec<String>,
        /// The login of who opened it.
        pub author: String,
        pub opened_at: SystemTime,
        pub comments: u32,
        /// Its web page.
        pub url: String,
    }

    /// An open pull request on a project's host, anyone's, as
    /// `ListProjectPullRequests` gives it.
    pub struct ProjectPullRequest {
        pub number: u64,
        pub title: String,
        /// Its web page.
        pub url: String,
        /// The login of who opened it.
        pub author: String,
        pub draft: bool,
        /// Opened by the account the host's CLI on the project's machine is
        /// signed in as; false when that account is unknown.
        pub mine: bool,
        pub created_at: SystemTime,
        pub updated_at: SystemTime,
    }

    /// The branch a pull request is made from, as `GetPullRequestHead` gives
    /// it.
    pub struct PullRequestHead {
        pub number: u64,
        /// Its name, without `origin/`.
        pub branch: String,
        /// The branch is in another repository, a fork, so `origin` lacks it.
        pub fork: bool,
    }

    /// An open pull request or issue of the caller's, as
    /// `ListProjectThreads` gives it.
    pub struct ProjectThread {
        pub kind: GitItemKind,
        /// `org/repo`.
        pub repo: String,
        pub number: u64,
        pub title: String,
        /// Its web page.
        pub url: String,
        /// Who wrote last and the start of what they wrote, on one line, such
        /// as `sam: Can we cut it down?`; empty when nobody has.
        pub last_other: String,
        /// Someone other than the caller (bots aside) wrote after the caller's
        /// last comment or review.
        pub needs_reply: bool,
        pub updated_at: SystemTime,
    }

    /// The chat about one pull request or issue, as `OpenThreadChat` gives it.
    pub struct ThreadChat {
        pub chat_id: Uuid,
        pub kind: GitItemKind,
        /// `org/repo`.
        pub repo: String,
        pub number: u64,
        pub title: String,
    }

    /// A pull request or issue as `GetProjectThread` reads it.
    pub struct ThreadView {
        pub kind: GitItemKind,
        pub number: u64,
        pub title: String,
        /// Its web page.
        pub url: String,
        /// The login of who opened it.
        pub author: String,
        /// The opening post, cut short when long.
        pub body: String,
        /// Someone other than the caller, bots aside, wrote after the caller.
        pub needs_reply: bool,
        /// The newest comments and reviews, oldest first.
        pub comments: Vec<ThreadPost>,
        /// How many older comments and reviews `comments` leaves out.
        pub earlier: u32,
    }

    /// A comment or review on a pull request or issue.
    pub struct ThreadPost {
        /// The writer's login.
        pub author: String,
        pub at: SystemTime,
        /// Cut short when long.
        pub body: String,
    }

    /// A pull request or issue as `GetThreadDetail` reads it. Its text is the
    /// host's, written by anyone: show it as data, never as markup.
    pub struct ThreadDetail {
        pub kind: GitItemKind,
        pub number: u64,
        pub title: String,
        /// Its web page.
        pub url: String,
        /// The login of who opened it.
        pub author: String,
        pub state: ThreadState,
        pub draft: bool,
        pub labels: Vec<String>,
        pub opened_at: SystemTime,
        /// The opening post, Markdown, cut short when long.
        pub body: String,
        /// Comments and reviews, oldest first; the newest when there are many.
        pub comments: Vec<ThreadPost>,
        /// How many older comments and reviews `comments` leaves out.
        pub earlier: u32,
        /// A pull request's review decision; `None` for an issue or where the
        /// host does not say.
        pub review: GitReview,
        /// How a pull request's checks stand on its head commit, where the host
        /// says.
        pub checks: Option<RunState>,
    }
}

ffi_enum! {
    /// Whether a pull request or issue is open.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    pub enum ThreadState {
        0 Open {}
        1 Closed {}
        2 Merged {}
    }
}

ffi_structs! {
    /// How `TrackProjectIssue` starts a worker on the issue.
    pub struct IssueStart {
        /// The coding agent: `claude`, `codex`, `gemini`, `pi` or `flsh`.
        pub agent: String,
        /// The `Machine::id` to run on; `None` picks one at start.
        pub node_id: Option<String>,
    }

    /// One task of a project.
    pub struct ProjectTask {
        pub id: Uuid,
        pub slug: String,
        pub title: String,
        pub body: String,
        pub state: ProjectTaskState,
        /// The task that must finish first.
        pub depends_on: Option<Uuid>,
        pub issue_number: Option<u64>,
        pub issue_url: Option<String>,
        pub branch: Option<String>,
        pub pr_url: Option<String>,
        pub created_at: SystemTime,
        pub updated_at: SystemTime,
    }
}

ffi_enum! {
    /// Where a project task stands.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "lowercase")]
    pub enum ProjectTaskState {
        0 Planned {}
        1 Running {}
        2 Blocked {}
        3 Review {}
        4 Done {}
        5 Dropped {}
    }
}

ffi_structs! {
    /// A coding agent working on a project task.
    pub struct ProjectWorker {
        pub id: Uuid,
        pub task_id: Uuid,
        /// The `Machine::id` it runs on.
        pub machine_id: String,
        pub machine_name: String,
        /// The agent, such as `claude` or `codex`.
        pub agent: String,
        /// The node's id for its session, once it has started.
        pub session: Option<String>,
        pub folder: String,
        pub state: ProjectWorkerState,
        /// The last thing it said, cut short.
        pub last_output: Option<String>,
        pub persona: Option<String>,
        pub started_at: SystemTime,
        pub ended_at: Option<SystemTime>,
    }
}

ffi_enum! {
    /// What a project worker is doing.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "lowercase")]
    pub enum ProjectWorkerState {
        0 Starting {}
        1 Running {}
        2 Waiting {}
        3 Ended {}
        4 Failed {}
    }
}

wire_structs! {
    /// A git host `CreateProject` can make a repository on, as
    /// `ListProjectForges` lists it.
    pub struct ProjectForge {
        /// `github.com`, or a Forgejo or Gitea server's host; what
        /// `NewRepository::host` takes.
        pub host: String,
        pub kind: GitHostKind,
        /// What `NewRepository::owner` takes: each account, then the
        /// organizations it can create in.
        pub owners: Vec<String>,
        /// Why `owners` is empty, for the user; `None` when the host answered.
        pub error: Option<String>,
    }

    /// The repository `CreateProject` makes on a git host.
    pub struct NewRepository {
        /// `ProjectForge::host`.
        pub host: String,
        /// One of `ProjectForge::owners`.
        pub owner: String,
        pub private: bool,
    }

    /// What `CreateProject` did.
    pub struct CreatedProject {
        /// `org/repo`, or `local/<name>` for a project with no git host.
        pub project: String,
        /// The repository's web page; empty for a project with no git host.
        pub url: String,
        /// Where it landed.
        pub imported: Vec<ImportedProject>,
        /// What went wrong on a machine; the repository stays.
        pub failed: Vec<CheckoutFailure>,
    }

    /// A machine `CreateProject` could not finish on.
    pub struct CheckoutFailure {
        pub machine_id: String,
        pub machine_name: String,
        /// Why, for the user.
        pub why: String,
    }
}
