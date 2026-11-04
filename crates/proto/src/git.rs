//! Git hosts and their issues and pull requests.

use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// One of the caller's GitHub accounts, as `ListGitHubAccounts` lists it.
/// The token stays on the server.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitHubAccount {
    /// The same id as its `Credential::id`.
    pub id: String,
    /// The login GitHub said the token signs in as.
    pub login: String,
    /// The orgs the token could see when it was saved.
    pub orgs: Vec<String>,
    /// The machines get it as `GH_TOKEN`, and projects no other account
    /// owns use it.
    pub default: bool,
    /// The token's last four characters.
    pub last4: String,
    pub created_at: SystemTime,
    /// The server can no longer open it; the user has to enter it again.
    pub unreadable: bool,
}

/// What `ListGitItems` found.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitItems {
    pub items: Vec<GitItem>,
    /// Every git host in the caller's credentials, whether it answered or
    /// not. Empty when the caller has none.
    pub hosts: Vec<GitHost>,
}

/// What `ListLabeledGitItems` found.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabeledGitItems {
    pub found: GitItems,
    /// The label names of each of `found.items`, in its order.
    pub labels: Vec<Vec<String>>,
}

/// One git host `ListGitItems` asked.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitHost {
    /// `github.com`, or a Forgejo server's host, such as `git.example.com`.
    pub host: String,
    pub kind: GitHostKind,
    /// Who the token signs in as, when known.
    pub account: Option<String>,
    /// Why its items are missing, for the user; `None` when it answered.
    pub error: Option<String>,
}

wire_enum! {
    /// What kind of server a git host is.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum GitHostKind {
        0 GitHub {}
        1 Forgejo {}
    }
}

/// An open pull request or issue, as `ListGitItems` lists it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitItem {
    /// As `GitHost::host`.
    pub host: String,
    /// `org/repo`.
    pub repo: String,
    pub number: u64,
    pub kind: GitItemKind,
    pub title: String,
    /// Its web page.
    pub url: String,
    pub updated_at: SystemTime,
    /// The caller opened it.
    pub authored: bool,
    /// The caller is assigned to it.
    pub assigned: bool,
    /// A pull request that asks for the caller's review.
    pub review_requested: bool,
    /// A draft pull request.
    pub draft: bool,
    /// How a pull request's reviews stand, where the host says.
    pub review: GitReview,
    /// A pull request whose checks fail on its head commit, where the host
    /// says.
    pub checks_failing: bool,
    pub comments: u32,
}

wire_enum! {
    /// Whether an item is a pull request or an issue.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum GitItemKind {
        0 PullRequest {}
        1 Issue {}
    }
}

wire_enum! {
    /// How a pull request's reviews stand. `None` for an issue, a PR with
    /// no deciding review yet, or a host that does not say.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum GitReview {
        0 None {}
        1 Approved {}
        2 ChangesRequested {}
    }
}
