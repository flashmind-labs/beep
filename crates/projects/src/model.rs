//! Names and paths the project modules share: repositories, machine paths, task slugs and hosts.

use beep_proto::node::agent;
pub use beep_proto::{ForgejoUrl, ForgejoUrlError};
use std::{borrow::Cow, fmt, str::FromStr};
use strum::{EnumString, IntoStaticStr};
use thiserror::Error;
use url::Url;

/// Longest owner or repository part of a [`RepoName`].
pub const MAX_REPO_PART: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RepoNameError {
    #[error("a repository is named owner/repo")]
    Shape,
    #[error(
        "each side of owner/repo is 1 to 100 letters, digits, '.', '_' or '-', and not '.' or '..'"
    )]
    Part,
}

/// A repository as `owner/repo`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepoName {
    name: String,
    /// Where the `/` is.
    slash: usize,
}

impl FromStr for RepoName {
    type Err = RepoNameError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
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
}

impl RepoName {
    pub fn owner(&self) -> &str {
        &self.name[..self.slash]
    }

    /// The repository part, without its owner.
    pub fn repo(&self) -> &str {
        &self.name[self.slash + 1..]
    }
}

impl fmt::Display for RepoName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    #[error("the path is not absolute")]
    Relative,
    #[error("the path has an empty, '.' or '..' part")]
    Segment,
    #[error("the path ends in '/'")]
    TrailingSlash,
}

/// An absolute machine path with no `.`, `..` or empty part and no trailing `/`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MachinePath(String);

impl FromStr for MachinePath {
    type Err = PathError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let rest = text.strip_prefix('/').ok_or(PathError::Relative)?;

        if rest.ends_with('/') || rest.is_empty() {
            return Err(PathError::TrailingSlash);
        }

        if rest.split('/').any(|part| matches!(part, "" | "." | "..")) {
            return Err(PathError::Segment);
        }

        Ok(Self(text.to_owned()))
    }
}

impl MachinePath {
    /// `rel` under this path; empty, `.` and `..` parts are dropped so it stays inside.
    pub fn join(&self, rel: &str) -> MachinePath {
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
    pub fn contains(&self, other: &MachinePath) -> bool {
        other
            .0
            .strip_prefix(&self.0)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    }
}

impl AsRef<str> for MachinePath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MachinePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A task's short name for its branch and folders: lowercase ASCII letters, digits and `-`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TaskSlug(String);

impl TaskSlug {
    /// The slug of `title` as `agent::slug` makes it; `task` when that is empty.
    pub fn from_title(title: &str) -> TaskSlug {
        let slug = agent::slug(title);
        if slug.is_empty() {
            TaskSlug("task".into())
        } else {
            TaskSlug(slug)
        }
    }

    /// `slug-n`, for a slug already taken.
    pub fn suffixed(&self, n: u32) -> TaskSlug {
        TaskSlug(format!("{}-{n}", self.0))
    }

    pub fn branch(&self) -> String {
        format!("beep/{}", self.0)
    }

    pub fn worktree(&self, checkout: &MachinePath) -> MachinePath {
        checkout.join(&format!(".worktrees/{}", self.0))
    }

    /// Where inputs land in the task's `worktree`.
    pub fn incoming(&self, worktree: &MachinePath) -> MachinePath {
        worktree.join(&format!(".beep/incoming/{}", self.0))
    }
}

impl AsRef<str> for TaskSlug {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("a task slug contains lowercase letters, digits and '-', with no leading or trailing '-'")]
pub struct TaskSlugError;

impl FromStr for TaskSlug {
    type Err = TaskSlugError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let valid = !text.is_empty()
            && !text.starts_with('-')
            && !text.ends_with('-')
            && text
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        valid
            .then(|| Self(Into::<String>::into(text)))
            .ok_or(TaskSlugError)
    }
}

impl fmt::Display for TaskSlug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether a project's repository is public, which limits what its issues and PRs may say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum Visibility {
    Public,
    Private,
}

/// A Forgejo server's hostname.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ForgejoHost(String);

#[derive(Debug, Error, PartialEq, Eq)]
#[error(
    "a Forgejo hostname contains letters, digits, '.' and '-', starting and ending with a letter or digit"
)]
pub struct ForgejoHostError;

impl FromStr for ForgejoHost {
    type Err = ForgejoHostError;

    /// A hostname of letters, digits, `.` and `-`, if `text` is one.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let valid = !text.is_empty()
            && !text.starts_with(['.', '-'])
            && !text.ends_with(['.', '-'])
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'));
        valid
            .then(|| Self(text.to_ascii_lowercase()))
            .ok_or(ForgejoHostError)
    }
}

impl AsRef<str> for ForgejoHost {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Where a project's issues or PRs live.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IssueHost {
    Github,
    Forgejo(ForgejoHost),
}

impl From<&IssueHost> for String {
    /// `github` or `forgejo:<host>`, as the store keeps it.
    fn from(host: &IssueHost) -> Self {
        match host {
            IssueHost::Github => "github".into(),
            IssueHost::Forgejo(host) => format!("forgejo:{}", host.as_ref()),
        }
    }
}

impl IssueHost {
    /// GitHub for `github.com` or `www.github.com`, else Forgejo.
    pub fn of_hostname(hostname: &str) -> Option<IssueHost> {
        let hostname = hostname.to_ascii_lowercase();

        if is_github(&hostname) {
            return Some(Self::Github);
        }

        hostname.parse().ok().map(Self::Forgejo)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("an issue host is 'github' or 'forgejo:<hostname>'")]
pub struct IssueHostError;

impl FromStr for IssueHost {
    type Err = IssueHostError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text == "github" {
            return Ok(Self::Github);
        }

        let host = text.strip_prefix("forgejo:").ok_or(IssueHostError)?;
        host.parse().map(Self::Forgejo).map_err(|_| IssueHostError)
    }
}

/// A remote URL's host, without port or user, and its path.
pub fn remote_parts(remote: &str) -> Option<(String, String)> {
    if remote.contains("://") {
        let url = Url::parse(remote).ok()?;
        return Some((url.host_str()?.to_owned(), url.path().to_owned()));
    }
    // scp-like: [user@]host:path
    let (host, path) = remote.split_once(':')?;
    let host = host.rsplit('@').next()?;
    Some((host.to_owned(), path.to_owned()))
}

/// "GitHub", or "Forgejo at <host>", as the agent and the user read it.
impl fmt::Display for IssueHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Github => f.write_str("GitHub"),
            Self::Forgejo(host) => write!(f, "Forgejo at {}", host.as_ref()),
        }
    }
}

/// The number at the end of a pull request's URL.
pub fn pr_number(url: &str) -> Option<u64> {
    url.trim_end_matches('/').rsplit('/').next()?.parse().ok()
}

/// Whether `hostname`, lowercased, is GitHub's.
pub fn is_github(hostname: &str) -> bool {
    matches!(hostname, "github.com" | "www.github.com")
}

/// Where a project files issues and opens PRs; `None` keeps that part in Beep only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Roles {
    pub issues: Option<IssueHost>,
    pub prs: Option<IssueHost>,
}

/// A task's issue on its host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueRef {
    pub number: u64,
    pub url: String,
}

/// The longest branch name taken.
const MAX_BRANCH: usize = 250;

/// A branch name git accepts: one line, not an option, no `..` or bad ref characters.
pub fn branch_name(text: &str) -> Option<&str> {
    let text = text.trim();
    let bad = text.is_empty()
        || text.len() > MAX_BRANCH
        || text.starts_with('-')
        || text.starts_with('/')
        || text.ends_with('/')
        || text.ends_with(".lock")
        || text.contains("..")
        || text.contains("//")
        || text.contains("@{")
        || text.contains(|c: char| {
            c.is_control()
                || c.is_whitespace()
                || matches!(c, ':' | '~' | '^' | '?' | '*' | '[' | '\\')
        });
    (!bad).then_some(text)
}

/// A Windows path without the verbatim prefix `canonicalize` gives it: `\\?\C:\x` is `C:\x` and
/// `\\?\UNC\host\share` is `\\host\share`. Any other path is returned as it is.
pub fn plain_path(path: &str) -> Cow<'_, str> {
    if let Some(share) = path.strip_prefix(r"\\?\UNC\") {
        Cow::Owned(format!(r"\\{share}"))
    } else if let Some(rest) = path.strip_prefix(r"\\?\")
        && rest.as_bytes().get(1) == Some(&b':')
    {
        Cow::Borrowed(rest)
    } else {
        Cow::Borrowed(path)
    }
}

/// A path the sh of Git for Windows prints, such as `/c/Users/me`, as Windows spells it:
/// `C:\Users\me`. `None` for a path that starts with no drive.
pub fn windows_path(msys: &str) -> Option<String> {
    let rest = msys.strip_prefix('/')?;
    let (drive, rest) = rest.split_once('/').unwrap_or((rest, ""));
    let mut letters = drive.chars();
    let (Some(letter), None) = (letters.next(), letters.next()) else {
        return None;
    };
    letter.is_ascii_alphabetic().then(|| {
        format!(
            "{}:\\{}",
            letter.to_ascii_uppercase(),
            rest.replace('/', "\\")
        )
    })
}

#[cfg(test)]
#[path = "tests/model.rs"]
mod tests;
