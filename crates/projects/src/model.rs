//! Names and paths the project modules share: repositories, machine paths and hosts.

pub use beep_proto::{ForgejoUrl, ForgejoUrlError};
use std::{borrow::Cow, fmt, str::FromStr};
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
        })
    }
}

impl RepoName {
    pub fn owner(&self) -> &str {
        self.name.split_once('/').map_or("", |(owner, _)| owner)
    }

    /// The repository part, without its owner.
    pub fn repo(&self) -> &str {
        self.name.split_once('/').map_or("", |(_, repo)| repo)
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

/// What [`github_owner_var`] names start with.
const GITHUB_OWNER_PREFIX: &str = "BEEP_GH_TOKEN_";

/// The env var for `owner`'s token when not the default account's, such as `BEEP_GH_TOKEN_ACME_LABS`.
pub fn github_owner_var(owner: &str) -> Option<String> {
    let valid = !owner.is_empty()
        && owner.len() <= 39
        && owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-');
    valid.then(|| {
        let name: String = owner
            .chars()
            .map(|c| match c {
                '-' => '_',
                c => c.to_ascii_uppercase(),
            })
            .collect();
        format!("{GITHUB_OWNER_PREFIX}{name}")
    })
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

/// Whether `hostname`, lowercased, is GitHub's.
pub fn is_github(hostname: &str) -> bool {
    matches!(hostname, "github.com" | "www.github.com")
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
