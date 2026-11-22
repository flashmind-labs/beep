//! Projects: their branches, imports, issues, threads, tasks and workers.

mod ci;
mod git;
pub mod groups;
mod sync;
mod workspace;

pub use ci::*;
pub use git::*;
pub use sync::*;
pub use workspace::*;

use crate::FoundAgent;
use std::time::SystemTime;
use uuid::Uuid;

pub(crate) mod epoch_ms {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    pub(crate) fn serialize<S: Serializer>(at: &SystemTime, s: S) -> Result<S::Ok, S::Error> {
        let ms = at
            .duration_since(UNIX_EPOCH)
            .map(|since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or_default();
        s.serialize_u64(ms)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<SystemTime, D::Error> {
        let ms = u64::deserialize(d)?;
        Ok(UNIX_EPOCH + Duration::from_millis(ms))
    }

    pub(crate) mod option {
        use super::*;
        use serde::Serialize;

        pub(crate) fn serialize<S: Serializer>(
            at: &Option<SystemTime>,
            s: S,
        ) -> Result<S::Ok, S::Error> {
            at.map(|t| {
                t.duration_since(UNIX_EPOCH)
                    .map(|since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
                    .unwrap_or_default()
            })
            .serialize(s)
        }

        pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
            d: D,
        ) -> Result<Option<SystemTime>, D::Error> {
            let ms = Option::<u64>::deserialize(d)?;
            Ok(ms.map(|ms| UNIX_EPOCH + Duration::from_millis(ms)))
        }
    }
}

fn is_github<S: serde::Serializer>(kind: &GitHostKind, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_bool(matches!(kind, GitHostKind::GitHub {}))
}

wire_structs! {
    /// A project's open pull requests and issues.
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

    /// `ProjectLists` with their labels.
    pub struct LabeledProjectLists {
        pub lists: ProjectLists,
        /// The label names of each of `lists.pull_requests`, in its order.
        pub pr_labels: Vec<Vec<String>>,
    }
}

wire_structs! {
    /// The user's fork of a repository to import.
    pub struct ImportFork {
        /// `org/repo`, the repository asked about.
        pub project: String,
        /// The caller's fork, `login/repo`; `None` when it is not cloned.
        pub fork: Option<String>,
        /// The head branches of the caller's open pull requests from the fork.
        pub branches: Vec<String>,
    }

    /// Folders the apps list projects under. A project is in a group when its folder on any machine is one of
    /// `paths` or inside one, both compared as `groups::project_group_path`
    /// gives them.
    pub struct ProjectGroup {
        pub id: Uuid,
        /// What the group is called: its first folder until renamed.
        pub name: String,
        /// The folders, such as `work`, oldest first.
        pub paths: Vec<String>,
    }

    /// A project's branches on one machine.
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
        #[serde(with = "epoch_ms")]
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

    /// What a look at a repository before importing it found.
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

    /// A branch with work, found before an import.
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
        #[serde(with = "epoch_ms::option")]
        pub committed_at: Option<SystemTime>,
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
        /// A user-selected name, independent of the project's identity.
        #[serde(default)]
        pub display_name: Option<String>,
    }

    /// A git checkout the user's coding agents worked in.
    pub struct FoundProject {
        /// `org/repo` from its remote, else the remote as it is.
        #[serde(rename = "name")]
        pub project: String,
        /// Its `origin` remote, what `ImportProject::repo` takes.
        pub remote: String,
        /// Where it is, one folder per machine.
        pub places: Vec<FoundPlace>,
        /// Sessions per agent, on all of its machines, most first.
        pub agents: Vec<FoundAgent>,
        /// When a session there last changed or its checked-out commit was
        /// made, whichever is newer.
        #[serde(with = "epoch_ms")]
        pub last_activity: SystemTime,
    }

    /// A repository on a git host.
    pub struct HostRepo {
        /// `org/repo`.
        pub name: String,
        /// Its web page, what `ImportProject::repo` takes.
        pub url: String,
        #[serde(with = "epoch_ms")]
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
        #[serde(rename = "id")]
        pub machine_id: String,
        #[serde(rename = "name")]
        pub machine_name: String,
        pub online: bool,
        /// The project's absolute folder there; `None` without a checkout.
        pub folder: Option<String>,
        /// Beep is looking through it for a checkout now.
        pub checking: bool,
        /// When Beep last looked through it, since the project was imported.
        #[serde(with = "epoch_ms::option")]
        pub checked_at: Option<SystemTime>,
    }

    /// An open issue on a project's issues host.
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
        /// A pull request is linked to it, to close it. GitHub only.
        #[serde(default)]
        pub linked_pr: bool,
    }

    /// An open pull request on a project's host, anyone's.
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

    /// The branch a pull request is made from.
    pub struct PullRequestHead {
        pub number: u64,
        /// Its name, without `origin/`.
        pub branch: String,
        /// The branch is in another repository, a fork, so `origin` lacks it.
        pub fork: bool,
    }

    /// A comment or review on a pull request or issue.
    pub struct ThreadPost {
        /// The writer's login.
        pub author: String,
        pub at: SystemTime,
        /// Cut short when long.
        pub body: String,
    }
}

/// One git host's repositories.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct HostRepos {
    /// `github.com`, or a Forgejo server's host, such as `git.example.com`.
    pub host: String,
    #[serde(rename = "github", serialize_with = "is_github")]
    pub kind: GitHostKind,
    /// Newest update first.
    pub repos: Vec<HostRepo>,
    /// Why `repos` is empty, for the user; `None` when the host answered.
    pub error: Option<String>,
}

/// A pull request or issue in detail. Its text is the
/// host's, written by anyone: show it as data, never as markup.
#[derive(Debug, Clone, PartialEq)]
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
    /// For an issue, the pull requests that will close it or mention it.
    /// GitHub only.
    pub linked_prs: Vec<LinkedPr>,
}

/// A pull request linked to an issue.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct LinkedPr {
    pub number: u64,
    pub title: String,
    /// Its web page, which names its repository.
    pub url: String,
    pub state: ThreadState,
}

/// Whether a pull request or issue is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum ThreadState {
    Open {},
    Closed {},
    Merged {},
}

impl serde::Serialize for ThreadState {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(Into::<&'static str>::into(*self))
    }
}

/// A git host `CreateProject` can make a repository on.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ProjectForge {
    /// `github.com`, or a Forgejo or Gitea server's host; what
    /// `NewRepository::host` takes.
    pub host: String,
    #[serde(rename = "github", serialize_with = "is_github")]
    pub kind: GitHostKind,
    /// What `NewRepository::owner` takes: each account, then the
    /// organizations it can create in.
    pub owners: Vec<String>,
    /// Why `owners` is empty, for the user; `None` when the host answered.
    pub error: Option<String>,
}

wire_structs! {
    /// The repository `CreateProject` makes on a git host.
    pub struct NewRepository {
        /// `ProjectForge::host`.
        pub host: String,
        /// One of `ProjectForge::owners`.
        pub owner: String,
        pub private: bool,
    }

    /// Where `CreateProject` puts the project on one machine.
    pub struct ProjectFolder {
        pub machine_id: String,
        /// The folder it goes under as `<folder>/<name>`, absolute or `~/...`.
        pub folder: String,
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

/// Makes a project called `name`. With `repository`, creates it on that git
/// host and clones it onto each of `machine_ids`; without, runs `git init`
/// on the one machine given. `folders` names the folder it goes under on a
/// machine; one not named takes its projects folder.
#[derive(Debug, Clone, PartialEq)]
pub struct CreateProject {
    pub name: String,
    pub repository: Option<NewRepository>,
    pub machine_ids: Vec<String>,
    pub folders: Vec<ProjectFolder>,
}
