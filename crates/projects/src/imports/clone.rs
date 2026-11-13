//! The shell script that clones a repository on a node, and what its output means.
//!
//! The clone lands in a hidden folder and is renamed into place, so an interrupted clone never
//! looks like a checkout. The token reaches git's credential helper as a shell variable, never in
//! a command line or saved remote. With a fork, the fork is `origin` and the repo `upstream`.

use super::{
    ImportError,
    fork::UsedFork,
    repo::{Host, RepoRef},
};
use crate::{Shell, quote, shell::tail_bytes};
use secrecy::{ExposeSecret, SecretString};
use std::time::Duration;

/// What the script prints last, before what it found.
const MARK: &str = "BEEP_IMPORT:";
/// How much of the output is kept to explain a failure.
const KEEP: usize = 8 * 1024;
/// How long a clone may take.
const CLONE_LIMIT: Duration = Duration::from_secs(30 * 60);

/// One clone to run.
pub struct Job<'a> {
    /// What git clones, such as `https://github.com/org/repo.git`.
    pub url: &'a str,
    /// The checkout's absolute folder.
    pub folder: &'a str,
    /// The user's token for the host, when they saved one.
    pub token: Option<&'a SecretString>,
    /// Whether `$GH_TOKEN` on the machine may stand in for `token`.
    pub github: bool,
    /// Set when `url` is the user's fork: what else the clone sets up.
    pub fork: Option<ForkSetup<'a>>,
}

/// What a clone of the user's fork sets up after cloning it.
#[derive(Clone, Copy)]
pub struct ForkSetup<'a> {
    /// What git fetches the repository itself from, as `upstream`.
    pub upstream_url: &'a str,
    /// The default branch the local one tracks as `upstream/<it>`; empty keeps the fork's.
    pub default_branch: &'a str,
    /// The user's pull request heads from the fork, made local branches tracking `origin`.
    pub branches: &'a [String],
}

/// The git helper that answers with `$BEEP_GIT_TOKEN`, and nothing when it is empty.
const HELPER: &str = r#"!f() { test "$1" = get && test -n "$BEEP_GIT_TOKEN" || return 0; echo username=x-access-token; echo "password=$BEEP_GIT_TOKEN"; }; f"#;

/// git with the user's token: only Beep's helper when given one, else the user's own (plus `$GH_TOKEN` on GitHub).
/// Secret: it holds the token.
fn git_with_token(token: Option<&SecretString>, github: bool) -> String {
    match (token, github) {
        (Some(token), _) => format!(
            "BEEP_GIT_TOKEN={} git -c credential.helper= -c {}",
            quote(token.expose_secret()),
            quote(&format!("credential.helper={HELPER}"))
        ),
        (None, true) => format!(
            "BEEP_GIT_TOKEN=\"${{GH_TOKEN:-}}\" git -c {}",
            quote(&format!("credential.helper={HELPER}"))
        ),
        (None, false) => "git".to_owned(),
    }
}

/// Makes each of `branches` a local branch in `$dir` tracking `<remote>/<branch>`, if missing.
fn track_branches(dir: &str, remote: &str, branches: &[String]) -> String {
    if branches.is_empty() {
        return String::new();
    }

    let quoted: Vec<String> = branches.iter().map(|branch| quote(branch)).collect();
    format!(
        "for b in {}; do git -C \"{dir}\" show-ref --verify --quiet \"refs/heads/$b\" || git -C \"{dir}\" branch --quiet --track \"$b\" {remote}/\"$b\" 2>/dev/null || true; done\n",
        quoted.join(" "),
        remote = quote(remote),
    )
}

/// Steps after cloning a fork into `$t`: fetch `upstream`, point the default branch at it, add PR branches.
fn fork_steps(git: &str, fork: &ForkSetup<'_>) -> String {
    let mut steps = format!(
        " && git -C \"$t\" remote add upstream {up} && {git} -C \"$t\" fetch --quiet upstream",
        up = quote(fork.upstream_url),
    );

    if !fork.default_branch.is_empty() {
        steps.push_str(&format!(
            " && git -C \"$t\" checkout --quiet -B {b} --track upstream/{b}",
            b = quote(fork.default_branch),
        ));
    }

    steps
}

/// The script. Secret: it holds the token.
pub fn script(job: &Job<'_>) -> SecretString {
    let (parent, leaf) = split(job.folder);
    let temp = format!("{parent}/.{leaf}.importing");
    let git = git_with_token(job.token, job.github);
    let (setup, branches) = match &job.fork {
        Some(fork) => (
            fork_steps(&git, fork),
            track_branches("$t", "origin", fork.branches),
        ),
        None => (String::new(), String::new()),
    };
    SecretString::from(format!(
        "d={folder}; p={parent}; t={temp}; u={url}\n\
         export GIT_TERMINAL_PROMPT=0 GCM_INTERACTIVE=never\n\
         if [ -e \"$d/.git\" ]; then\n\
         \x20 printf '\\n{MARK} existing %s\\n' \"$(git -C \"$d\" remote get-url origin 2>/dev/null)\"\n\
         elif [ -n \"$(ls -A \"$d\" 2>/dev/null)\" ]; then\n\
         \x20 printf '\\n{MARK} occupied\\n'\n\
         elif ! command -v git >/dev/null 2>&1; then\n\
         \x20 printf '\\n{MARK} nogit\\n'\n\
         elif mkdir -p \"$p\" && rm -rf \"$t\" && {git} clone --quiet -- \"$u\" \"$t\"{setup}; then\n\
         {branches}\
         \x20 rmdir \"$d\" 2>/dev/null; mv \"$t\" \"$d\" && printf '\\n{MARK} cloned\\n'\n\
         else\n\
         \x20 s=$?; rm -rf \"$t\"; (exit $s)\n\
         fi\n",
        folder = quote(job.folder),
        parent = quote(parent),
        temp = quote(&temp),
        url = quote(job.url),
    ))
}

/// A remote a checkout gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRemote {
    pub name: String,
    pub url: String,
}

/// A checkout already there, and the remotes it lacks for the fork.
pub struct Link<'a> {
    /// The checkout's absolute folder.
    pub folder: &'a str,
    pub token: Option<&'a SecretString>,
    pub github: bool,
    pub add: &'a [NewRemote],
    /// The remote that is the fork, which the branches track.
    pub fork_remote: &'a str,
    /// The remote that is the repository itself.
    pub upstream_remote: &'a str,
    /// The heads of the user's pull requests from the fork.
    pub branches: &'a [String],
}

/// Adds `link`'s remotes to an existing checkout, fetches, and makes PR branches local; prints `linked`.
/// Secret: it holds the token.
pub fn link_script(link: &Link<'_>) -> SecretString {
    let git = git_with_token(link.token, link.github);
    let mut script = format!(
        "d={folder}\nexport GIT_TERMINAL_PROMPT=0 GCM_INTERACTIVE=never\n",
        folder = quote(link.folder)
    );

    for remote in link.add {
        script.push_str(&format!(
            "git -C \"$d\" remote add {} {} || exit 1\n",
            quote(&remote.name),
            quote(&remote.url)
        ));
    }

    script.push_str(&format!(
        "{git} -C \"$d\" fetch --quiet {fork} && {git} -C \"$d\" fetch --quiet {up} || exit 1\n",
        fork = quote(link.fork_remote),
        up = quote(link.upstream_remote),
    ));
    script.push_str(&track_branches("$d", link.fork_remote, link.branches));
    script.push_str(&format!("printf '\\n{MARK} linked\\n'\n"));
    SecretString::from(script)
}

/// Whether the link script got to the end.
pub fn linked(output: &str, exit_code: Option<i32>) -> bool {
    exit_code == Some(0)
        && output
            .lines()
            .rev()
            .find_map(|line| line.trim().strip_prefix(MARK))
            .is_some_and(|said| said.trim() == "linked")
}

/// The folder above `folder`, and its last part.
fn split(folder: &str) -> (&str, &str) {
    match folder.rfind(['/', '\\']) {
        Some(at) => (&folder[..at], &folder[at + 1..]),
        None => (".", folder),
    }
}

/// What the script found.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Cloned,
    /// A checkout was there already, with this `origin` (empty for none).
    Existing(String),
    /// The folder holds other files.
    Occupied,
    NoGit,
    TimedOut,
    /// git failed; the end of what it said.
    Failed(String),
}

/// Reads the script's output and how it ended.
pub fn outcome(output: &str, exit_code: Option<i32>, timed_out: bool) -> Outcome {
    if timed_out {
        return Outcome::TimedOut;
    }

    let said = output
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix(MARK))
        .map(str::trim);
    match (said, exit_code) {
        (Some("cloned"), Some(0)) => Outcome::Cloned,
        (Some("occupied"), _) => Outcome::Occupied,
        (Some("nogit"), _) => Outcome::NoGit,
        (Some(rest), _) if rest.starts_with("existing") => {
            Outcome::Existing(rest["existing".len()..].trim().to_owned())
        }
        _ => Outcome::Failed(tail(output)),
    }
}

/// The end of `output`, up to `KEEP` bytes.
pub fn tail(output: &str) -> String {
    tail_bytes(output, KEEP).to_owned()
}

/// What went wrong, for a clone git refused.
pub struct Failure<'a> {
    pub output: &'a str,
    pub project: String,
    pub host: String,
    pub machine: &'a str,
    pub had_token: bool,
}

/// Git's own HTTP 404 line: `fatal: repository 'https://host/o/r/' not found`.
fn quoted_repository_missing(text: &str) -> bool {
    text.lines().any(|line| {
        line.split_once("repository '")
            .is_some_and(|(_, rest)| rest.contains("' not found"))
    })
}

/// The plain sentence for a failed clone.
pub fn explain(failure: Failure<'_>) -> ImportError {
    let text = failure.output.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|needle| text.contains(needle));
    if has(&[
        "repository not found",
        "remote: not found",
        "could not read username",
        "authentication failed",
        "terminal prompts disabled",
        "returned error: 401",
        "returned error: 403",
        "returned error: 404",
        "does not appear to be a git repository",
    ]) || quoted_repository_missing(&text)
    {
        return if failure.had_token {
            ImportError::NoAccess {
                repo: failure.project,
                host: failure.host,
            }
        } else {
            ImportError::NotFound {
                repo: failure.project,
                host: failure.host,
            }
        };
    }
    if has(&[
        "could not resolve host",
        "failed to connect",
        "connection timed out",
        "connection refused",
        "network is unreachable",
    ]) {
        return ImportError::Unreachable {
            machine: failure.machine.to_owned(),
            host: failure.host,
        };
    }
    if has(&["no space left"]) {
        return ImportError::NoSpace(failure.machine.to_owned());
    }

    let detail = failure
        .output
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or("git gave no reason")
        .trim_start_matches("fatal: ")
        .to_owned();
    ImportError::Failed {
        machine: failure.machine.to_owned(),
        detail,
    }
}

/// Clones on `shell`'s machine, named `machine`, the fork when there is one; `false` when a
/// checkout was there already.
pub async fn run_clone(
    shell: &dyn Shell,
    machine: &str,
    repo: &RepoRef,
    fork: Option<&UsedFork>,
    folder: &str,
    token: Option<&SecretString>,
) -> Result<bool, ImportError> {
    let cloning = fork.map_or(repo, |fork| &fork.repo);
    let url = cloning.clone_url();
    let upstream_url = repo.clone_url();
    let script = script(&Job {
        url: &url,
        folder,
        token,
        github: repo.host == Host::GitHub,
        fork: fork.map(|fork| ForkSetup {
            upstream_url: &upstream_url,
            default_branch: &fork.found.default_branch,
            branches: &fork.found.branches,
        }),
    });
    let ran = shell
        .run(script.expose_secret(), CLONE_LIMIT)
        .await
        .map_err(|e| ImportError::Link {
            machine: machine.to_owned(),
            detail: format!("{e:#}"),
        })?;
    match outcome(&ran.output, ran.exit_code, ran.timed_out) {
        Outcome::Cloned => Ok(true),
        Outcome::Existing(remote)
            if repo.matches_remote(&remote)
                || fork.is_some_and(|fork| fork.repo.matches_remote(&remote)) =>
        {
            Ok(false)
        }
        Outcome::Existing(remote) => Err(ImportError::OtherRepo {
            folder: folder.to_owned(),
            machine: machine.to_owned(),
            remote: if remote.is_empty() {
                "no origin remote".to_owned()
            } else {
                remote
            },
        }),
        Outcome::Occupied => Err(ImportError::Occupied {
            folder: folder.to_owned(),
            machine: machine.to_owned(),
        }),
        Outcome::NoGit => Err(ImportError::NoGit(machine.to_owned())),
        Outcome::TimedOut => Err(ImportError::TimedOut(cloning.project())),
        Outcome::Failed(said) => Err(explain(Failure {
            output: &said,
            project: cloning.project(),
            host: repo.host_name(),
            machine,
            had_token: token.is_some(),
        })),
    }
}

#[cfg(test)]
#[path = "../tests/imports_clone.rs"]
mod tests;
