//! Names and paths the project modules share: repositories, machine paths, task slugs and hosts.

use beep_proto::node::agent;
use std::{fmt, str::FromStr};
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

impl RepoName {
    pub fn parse(text: &str) -> Result<Self, RepoNameError> {
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

impl MachinePath {
    pub fn parse(text: &str) -> Result<Self, PathError> {
        let rest = text.strip_prefix('/').ok_or(PathError::Relative)?;
        if rest.ends_with('/') || rest.is_empty() {
            return Err(PathError::TrailingSlash);
        }
        if rest.split('/').any(|part| matches!(part, "" | "." | "..")) {
            return Err(PathError::Segment);
        }
        Ok(Self(text.to_owned()))
    }

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

    pub fn as_str(&self) -> &str {
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

    /// A slug as stored, if it is one.
    pub fn parse(text: &str) -> Option<TaskSlug> {
        let valid = !text.is_empty()
            && !text.starts_with('-')
            && !text.ends_with('-')
            && text
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        valid.then(|| TaskSlug(text.into()))
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

    pub fn as_str(&self) -> &str {
        &self.0
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

impl ForgejoHost {
    /// A hostname of letters, digits, `.` and `-`, if `text` is one.
    pub fn parse(text: &str) -> Option<ForgejoHost> {
        let valid = !text.is_empty()
            && !text.starts_with(['.', '-'])
            && !text.ends_with(['.', '-'])
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'));
        valid.then(|| ForgejoHost(text.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why text is not a Forgejo URL, for the user.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("{0}")]
pub struct ForgejoUrlError(pub &'static str);

/// A Forgejo or Gitea base URL: https (http only on the user's network), no query, fragment or trailing slash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgejoUrl(Url);

impl ForgejoUrl {
    pub fn as_str(&self) -> &str {
        self.0.as_str().trim_end_matches('/')
    }

    /// The parsed URL.
    pub fn url(&self) -> &Url {
        &self.0
    }

    /// Its host name alone, without the port.
    pub fn hostname(&self) -> &str {
        self.0.host_str().unwrap_or_default()
    }

    /// Its host, with the port when it has one, such as `git.example.com`.
    pub fn host(&self) -> String {
        let host = self.0.host_str().unwrap_or_default();
        match self.0.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        }
    }
}

impl FromStr for ForgejoUrl {
    type Err = ForgejoUrlError;

    fn from_str(text: &str) -> Result<Self, ForgejoUrlError> {
        let text = text.trim();
        let mut url = Url::parse(text).map_err(|_| {
            ForgejoUrlError("The server URL is not a URL, such as https://git.example.com.")
        })?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(ForgejoUrlError(
                "The server URL must start with https:// or http://.",
            ));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(ForgejoUrlError(
                "Put the token in its own field, not in the URL.",
            ));
        }
        url.set_query(None);
        url.set_fragment(None);
        // Pasted from the browser: the API sits at the server's root or
        // under its sub-path, never under `/api/v1` itself.
        let path = url.path().trim_end_matches('/').to_owned();
        let path = path.strip_suffix("/api/v1").unwrap_or(&path).to_owned();
        url.set_path(&path);
        Ok(Self(url))
    }
}

impl fmt::Display for ForgejoUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a project's issues or PRs live.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IssueHost {
    Github,
    Forgejo(ForgejoHost),
}

impl IssueHost {
    /// `github` or `forgejo:<host>`, as the store keeps it.
    pub fn stored(&self) -> String {
        match self {
            Self::Github => "github".into(),
            Self::Forgejo(host) => format!("forgejo:{}", host.as_str()),
        }
    }

    /// GitHub for `github.com` or `www.github.com`, else Forgejo.
    pub fn of_hostname(hostname: &str) -> Option<IssueHost> {
        let hostname = hostname.to_ascii_lowercase();
        if is_github(&hostname) {
            return Some(Self::Github);
        }
        ForgejoHost::parse(&hostname).map(Self::Forgejo)
    }

    pub fn from_stored(text: &str) -> Option<IssueHost> {
        if text == "github" {
            return Some(Self::Github);
        }
        let host = text.strip_prefix("forgejo:")?;
        ForgejoHost::parse(host).map(Self::Forgejo)
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
            Self::Forgejo(host) => write!(f, "Forgejo at {}", host.as_str()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_names_parse() {
        let name = RepoName::parse("acme/widget").expect("a valid name");
        assert_eq!(name.to_string(), "acme/widget");
        assert_eq!(name.repo(), "widget");
        for bad in [
            "acme", "a/b/c", "../x", "acme/", "ac me/x", "./x", "acme/..",
        ] {
            assert!(RepoName::parse(bad).is_err(), "{bad} should not parse");
        }
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
    fn branch_names_git_refuses_are_refused() {
        assert_eq!(branch_name(" beep/login "), Some("beep/login"));
        for bad in [
            "", "-x", "a b", "a..b", "a:b", "a~1", "/a", "a/", "x.lock", "a\nb",
        ] {
            assert_eq!(branch_name(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn forgejo_urls_are_made_canonical() {
        let url: ForgejoUrl = " https://git.example.com/api/v1/?x=1 ".parse().unwrap();
        assert_eq!(url.as_str(), "https://git.example.com");
        let url: ForgejoUrl = "https://code.example.org:3000/forge/".parse().unwrap();
        assert_eq!(url.as_str(), "https://code.example.org:3000/forge");
        assert_eq!(url.host(), "code.example.org:3000");
        let url: ForgejoUrl = "http://forge.local:3000".parse().unwrap();
        assert_eq!(url.as_str(), "http://forge.local:3000");
        // Over a VPN a server named like one on the internet can be plain http.
        let url: ForgejoUrl = "http://git.example.com:8087".parse().unwrap();
        assert_eq!(url.as_str(), "http://git.example.com:8087");
        assert!("ftp://git.example.com".parse::<ForgejoUrl>().is_err());
        assert!("git.example.com".parse::<ForgejoUrl>().is_err());
        assert!(
            "https://me:pw@git.example.com"
                .parse::<ForgejoUrl>()
                .is_err()
        );
    }
}
