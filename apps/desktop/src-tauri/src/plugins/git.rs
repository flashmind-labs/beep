//! Reaching a plugin's repository with the user's `git`: what may be asked for
//! (`RepoUrl`, `GitRef`, `Subfolder`) and the two things asked of git, a shallow
//! fetch into a folder and a look at a branch's newest commit.

use std::ffi::OsStr;
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use beep_plugins::Commit;
use thiserror::Error;
use tokio::process::Command;
use tokio::time::timeout;
use url::Url;

/// How long a fetch may take.
const FETCH_LIMIT: Duration = Duration::from_secs(120);
/// How long a quick look at a remote may take.
const LOOK_LIMIT: Duration = Duration::from_secs(30);
const MAX_TEXT: usize = 300;

#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum InvalidInput {
    #[error(
        "Use owner/repo, or a full https or ssh address such as https://github.com/owner/repo.git."
    )]
    Repository,
    #[error(
        "That branch, tag or commit name is not valid. Use letters, digits, dots, dashes, underscores and slashes."
    )]
    Reference,
    #[error("The folder must be a path inside the repository, such as plugins/cat.")]
    Folder,
}

/// A repository address git may fetch from: https or ssh, never a local path
/// or another transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepoUrl(String);

impl RepoUrl {
    /// `owner/repo` becomes a GitHub address; https and ssh addresses (and the
    /// `git@host:owner/repo` form of ssh) are kept; anything else is refused.
    pub(crate) fn parse(text: &str) -> Result<Self, InvalidInput> {
        let text = text.trim();
        if text.is_empty()
            || text.starts_with('-')
            || text.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return Err(InvalidInput::Repository);
        }

        if text.starts_with("https://") || text.starts_with("ssh://") {
            return Self::remote(text);
        }
        if let Some(short) = short_name(text) {
            return Ok(Self(format!("https://github.com/{short}.git")));
        }
        if let Some(address) = scp_form(text) {
            return Self::remote(&address);
        }

        Err(InvalidInput::Repository)
    }

    /// An address an installed plugin recorded, which was checked when it was installed.
    pub(crate) fn stored(text: &str) -> Result<Self, InvalidInput> {
        #[cfg(test)]
        if text.starts_with("file://") {
            return Ok(Self(text.to_owned()));
        }

        Self::parse(text)
    }

    fn remote(text: &str) -> Result<Self, InvalidInput> {
        let url = Url::parse(text).map_err(|_| InvalidInput::Repository)?;
        let named_host = url.host_str().is_some_and(|host| !host.is_empty());
        let plain = url.password().is_none() && url.query().is_none() && url.fragment().is_none();
        let has_path = url.path().trim_matches('/').len() > 1;
        let scheme_ok = matches!(url.scheme(), "https" | "ssh");

        if scheme_ok && named_host && plain && has_path {
            Ok(Self(text.to_owned()))
        } else {
            Err(InvalidInput::Repository)
        }
    }

    /// Where a repository is, for git and for people.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// A local repository, for tests only: the shipped parser never accepts one.
    #[cfg(test)]
    pub(crate) fn local_for_tests(path: &Path) -> Self {
        Self(format!("file://{}", path.display()))
    }
}

impl fmt::Display for RepoUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// `owner/repo`: two plain names.
fn short_name(text: &str) -> Option<&str> {
    let (owner, repo) = text.split_once('/')?;
    let plain = |part: &str| {
        let leading = part
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric());
        leading
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
            && !part.contains("..")
    };

    (plain(owner) && plain(repo.trim_end_matches(".git"))).then_some(text.trim_end_matches(".git"))
}

/// `user@host:path` as an ssh address.
fn scp_form(text: &str) -> Option<String> {
    let (user_host, path) = text.split_once(':')?;
    let (user, host) = user_host.split_once('@')?;
    let name = |part: &str, extra: &[char]| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || extra.contains(&c))
    };
    let path = path.trim_start_matches('/');

    (name(user, &['.', '-', '_'])
        && name(host, &['.', '-'])
        && !path.is_empty()
        && !path.starts_with(':'))
    .then(|| format!("ssh://{user}@{host}/{path}"))
}

/// A branch, tag or commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GitRef(String);

impl GitRef {
    pub(crate) fn parse(text: &str) -> Result<Self, InvalidInput> {
        let text = text.trim();
        let plain = text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '/' | '+'));
        let shaped = !text.is_empty()
            && text.len() <= 200
            && !text.starts_with(['-', '/', '.'])
            && !text.ends_with(['/', '.'])
            && !text.contains("..")
            && !text.contains("//")
            && !text.ends_with(".lock");

        if plain && shaped {
            Ok(Self(text.to_owned()))
        } else {
            Err(InvalidInput::Reference)
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether it reads as a commit id, which a remote cannot be asked about.
    fn is_commit(&self) -> bool {
        self.0.parse::<Commit>().is_ok()
    }
}

/// The folder of a repository that holds the plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Subfolder(PathBuf);

impl Subfolder {
    pub(crate) fn parse(text: &str) -> Result<Self, InvalidInput> {
        let path = Path::new(text.trim());
        let mut clean = PathBuf::new();
        for part in path.components() {
            match part {
                Component::Normal(name) if !name.to_string_lossy().contains('\\') => {
                    clean.push(name);
                }
                Component::CurDir => {}
                _ => return Err(InvalidInput::Folder),
            }
        }

        if clean.as_os_str().is_empty() || clean.as_os_str().len() > 200 {
            return Err(InvalidInput::Folder);
        }

        Ok(Self(clean))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

#[derive(Debug, Error)]
pub(crate) enum GitError {
    #[error("Git is not installed. Install Git, then try again.")]
    Missing,
    #[error("Git took too long. Check the address and your connection, then try again.")]
    TimedOut,
    #[error(
        "Git could not read that repository. It may not exist, or it may be private and need a sign-in."
    )]
    NotFound,
    #[error("The repository has no branch, tag or commit named {0:?}.")]
    RefNotFound(String),
    #[error("Git failed: {0}")]
    Failed(String),
}

/// What a plugin's repository holds right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Remote {
    /// The commit a branch or tag points at.
    At(Commit),
    /// The reference is a commit, so nothing newer exists to find.
    Pinned,
}

/// The user's `git`, run with no shell, no prompt and no hooks.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Git {
    /// Whether git may read a local repository by `file://`. Only tests set it.
    allow_file: bool,
}

impl Git {
    pub(crate) fn new() -> Self {
        Self { allow_file: false }
    }

    #[cfg(test)]
    pub(crate) fn allowing_files_for_tests() -> Self {
        Self { allow_file: true }
    }

    /// Fetches one commit of a repository into a new folder, with no history,
    /// tags or submodules, and checks it out. Returns its commit.
    pub(crate) async fn fetch(
        &self,
        url: &RepoUrl,
        reference: Option<&GitRef>,
        into: &Path,
    ) -> Result<Commit, GitError> {
        let wanted = reference.map_or("HEAD", GitRef::as_str);
        let cwd = into.parent().unwrap_or(into);

        self.run(
            cwd,
            FETCH_LIMIT,
            [OsStr::new("init"), OsStr::new("-q"), into.as_os_str()],
        )
        .await?;
        self.run(
            into,
            FETCH_LIMIT,
            [
                "fetch",
                "-q",
                "--depth",
                "1",
                "--no-tags",
                "--no-recurse-submodules",
                "--",
                url.as_str(),
                wanted,
            ]
            .map(OsStr::new),
        )
        .await
        .map_err(|error| match error {
            GitError::Failed(text) if text.contains("couldn't find remote ref") => {
                GitError::RefNotFound(wanted.to_owned())
            }
            other => other,
        })?;
        self.run(
            into,
            FETCH_LIMIT,
            ["checkout", "-q", "--detach", "FETCH_HEAD"].map(OsStr::new),
        )
        .await?;

        let head = self
            .run(into, LOOK_LIMIT, ["rev-parse", "HEAD"].map(OsStr::new))
            .await?;

        head.trim()
            .parse()
            .map_err(|_| GitError::Failed("git did not name the commit".to_owned()))
    }

    /// The commit a branch, tag or the default branch points at on the remote.
    pub(crate) async fn newest(
        &self,
        url: &RepoUrl,
        reference: Option<&GitRef>,
    ) -> Result<Remote, GitError> {
        let patterns: Vec<String> = match reference {
            None => vec!["HEAD".to_owned()],
            Some(name) => vec![
                format!("refs/heads/{}", name.as_str()),
                format!("refs/tags/{}", name.as_str()),
                format!("refs/tags/{}^{{}}", name.as_str()),
            ],
        };
        let mut args = vec![
            "ls-remote".to_owned(),
            "--".to_owned(),
            url.as_str().to_owned(),
        ];
        args.extend(patterns);

        let listed = self
            .run(Path::new("."), LOOK_LIMIT, args.iter().map(OsStr::new))
            .await?;

        match pick(&listed) {
            Some(commit) => Ok(Remote::At(commit)),
            None if reference.is_some_and(GitRef::is_commit) => Ok(Remote::Pinned),
            None => Err(GitError::RefNotFound(
                reference.map_or("HEAD", GitRef::as_str).to_owned(),
            )),
        }
    }

    async fn run<I, S>(&self, cwd: &Path, limit: Duration, args: I) -> Result<String, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.run_with_program("git", cwd, limit, args).await
    }

    /// Runs `program` as git; tests name one that does not exist.
    async fn run_with_program<I, S>(
        &self,
        program: &str,
        cwd: &Path,
        limit: Duration,
        args: I,
    ) -> Result<String, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = Command::new(program);
        command
            .current_dir(cwd)
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsmonitor=false",
            ])
            .args(["-c", "protocol.allow=never"])
            .args(["-c", "protocol.https.allow=always"])
            .args(["-c", "protocol.ssh.allow=always"]);
        if self.allow_file {
            command.args(["-c", "protocol.file.allow=always"]);
        }
        for name in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_COMMON_DIR",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        ] {
            command.env_remove(name);
        }

        let output = command
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .output();

        let output = match timeout(limit, output).await {
            Err(_) => return Err(GitError::TimedOut),
            Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(GitError::Missing);
            }
            Ok(Err(error)) => return Err(GitError::Failed(error.to_string())),
            Ok(Ok(output)) => output,
        };

        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
        }

        Err(failure(&String::from_utf8_lossy(&output.stderr)))
    }
}

/// The commit of the best line of `git ls-remote`: a tag's own commit, then a
/// branch, then a tag object, then `HEAD`.
fn pick(listed: &str) -> Option<Commit> {
    let lines: Vec<(&str, &str)> = listed
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .collect();
    let by = |keep: fn(&str) -> bool| {
        lines
            .iter()
            .find(|(_, name)| keep(name))
            .and_then(|(sha, _)| sha.parse().ok())
    };

    by(|name| name.ends_with("^{}"))
        .or_else(|| by(|name| name.starts_with("refs/heads/")))
        .or_else(|| by(|name| name.starts_with("refs/tags/")))
        .or_else(|| by(|name| name == "HEAD"))
}

/// Reads git's complaint into the error a person can act on.
fn failure(stderr: &str) -> GitError {
    let lower = stderr.to_lowercase();
    let unreadable = [
        "repository not found",
        "does not appear to be a git repository",
        "could not read username",
        "authentication failed",
        "permission denied",
        "not found",
    ];
    if unreadable.iter().any(|text| lower.contains(text)) {
        return GitError::NotFound;
    }

    let last = stderr
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("it gave no reason");

    GitError::Failed(last.chars().take(MAX_TEXT).collect())
}

#[cfg(test)]
#[path = "../tests/plugins_git.rs"]
mod tests;
