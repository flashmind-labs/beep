//! The shell script that clones a repository on a node, and what its output means.
//!
//! The clone lands in a hidden folder and is renamed into place, so an interrupted clone never
//! looks like a checkout. The token reaches git's credential helper as a shell variable, never in
//! a command line or saved remote. With a fork, the fork is `origin` and the repo `upstream`.

use super::ImportError;
use secrecy::{ExposeSecret, SecretString};

/// What the script prints last, before what it found.
const MARK: &str = "BEEP_IMPORT:";
/// How much of the output is kept to explain a failure.
const KEEP: usize = 8 * 1024;

/// One clone to run.
pub(crate) struct Job<'a> {
    /// What git clones, such as `https://github.com/org/repo.git`.
    pub(crate) url: &'a str,
    /// The checkout's absolute folder.
    pub(crate) folder: &'a str,
    /// The user's token for the host, when they saved one.
    pub(crate) token: Option<&'a SecretString>,
    /// Whether `$GH_TOKEN` on the machine may stand in for `token`.
    pub(crate) github: bool,
    /// Set when `url` is the user's fork: what else the clone sets up.
    pub(crate) fork: Option<ForkSetup<'a>>,
}

/// What a clone of the user's fork sets up after cloning it.
#[derive(Clone, Copy)]
pub(crate) struct ForkSetup<'a> {
    /// What git fetches the repository itself from, as `upstream`.
    pub(crate) upstream_url: &'a str,
    /// The default branch the local one tracks as `upstream/<it>`; empty keeps the fork's.
    pub(crate) default_branch: &'a str,
    /// The user's pull request heads from the fork, made local branches tracking `origin`.
    pub(crate) branches: &'a [String],
}

/// Quotes `text` for sh.
pub(super) fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
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
pub(crate) fn script(job: &Job<'_>) -> SecretString {
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
pub(crate) struct NewRemote {
    pub(crate) name: String,
    pub(crate) url: String,
}

/// A checkout already there, and the remotes it lacks for the fork.
pub(crate) struct Link<'a> {
    /// The checkout's absolute folder.
    pub(crate) folder: &'a str,
    pub(crate) token: Option<&'a SecretString>,
    pub(crate) github: bool,
    /// Remotes to add.
    pub(crate) add: &'a [NewRemote],
    /// The remote that is the fork, which the branches track.
    pub(crate) fork_remote: &'a str,
    /// The remote that is the repository itself.
    pub(crate) upstream_remote: &'a str,
    /// The heads of the user's pull requests from the fork.
    pub(crate) branches: &'a [String],
}

/// Adds `link`'s remotes to an existing checkout, fetches, and makes PR branches local; prints `linked`.
/// Secret: it holds the token.
pub(crate) fn link_script(link: &Link<'_>) -> SecretString {
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
pub(crate) fn linked(output: &str, exit_code: Option<i32>) -> bool {
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
pub(crate) enum Outcome {
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
pub(crate) fn outcome(output: &str, exit_code: Option<i32>, timed_out: bool) -> Outcome {
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
pub(crate) fn tail(output: &str) -> String {
    let output = output.trim();
    let mut start = output.len().saturating_sub(KEEP);
    while !output.is_char_boundary(start) {
        start += 1;
    }
    output[start..].to_owned()
}

/// What went wrong, for a clone git refused.
pub(crate) struct Failure<'a> {
    pub(crate) output: &'a str,
    pub(crate) project: String,
    pub(crate) host: String,
    pub(crate) machine: &'a str,
    pub(crate) had_token: bool,
}

/// The plain sentence for a failed clone.
pub(crate) fn explain(failure: Failure<'_>) -> ImportError {
    let text = failure.output.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|needle| text.contains(needle));
    if has(&[
        "repository not found",
        "not found",
        "could not read username",
        "authentication failed",
        "terminal prompts disabled",
        "403",
        "401",
        "does not appear to be a git repository",
    ]) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{path::Path, process::Command};

    #[test]
    fn the_token_stays_out_of_the_url_and_command_lines() {
        let token = SecretString::from("tok_abc'123");
        let script = script(&Job {
            url: "https://git.example.com/o/r.git",
            folder: "/w/projects/o/r",
            token: Some(&token),
            github: false,
            fork: None,
        });
        let text = script.expose_secret();
        assert!(text.contains(r"BEEP_GIT_TOKEN='tok_abc'\''123' git -c credential.helper= -c"));
        assert!(text.contains("u='https://git.example.com/o/r.git'"));
        assert!(text.contains("t='/w/projects/o/.r.importing'"));
    }

    /// Runs the script against real git as a node's shell does: `exec 2>&1`, then `eval`.
    fn run(script: &SecretString, home: &Path) -> (String, Option<i32>) {
        let out = Command::new("bash")
            .arg("-c")
            .arg("exec 2>&1; eval \"$BEEP_SCRIPT\" < /dev/null")
            .env("BEEP_SCRIPT", script.expose_secret())
            .env("HOME", home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("GH_TOKEN")
            .output()
            .expect("bash runs");
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            out.status.code(),
        )
    }

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", dir)
            .output()
            .expect("git runs")
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    fn git_out(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", dir)
            .output()
            .expect("git runs");
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    fn commit(dir: &Path, message: &str) {
        git(
            dir,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.com",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                message,
            ],
        );
    }

    /// A bare upstream with `main`, and a bare fork behind it with a PR branch `fix-retry`.
    fn upstream_and_fork(root: &Path) -> (String, String) {
        let work = root.join("work");
        std::fs::create_dir(&work).unwrap();
        git(&work, &["init", "-q", "-b", "main"]);
        commit(&work, "first");
        let fork = root.join("fork.git");
        let upstream = root.join("upstream.git");
        git(
            root,
            &[
                "clone",
                "-q",
                "--bare",
                &work.to_string_lossy(),
                &fork.to_string_lossy(),
            ],
        );
        commit(&work, "second, upstream only");
        git(
            root,
            &[
                "clone",
                "-q",
                "--bare",
                &work.to_string_lossy(),
                &upstream.to_string_lossy(),
            ],
        );
        git(&work, &["checkout", "-q", "-b", "fix-retry", "HEAD~1"]);
        commit(&work, "the fix");
        git(&work, &["push", "-q", &fork.to_string_lossy(), "fix-retry"]);
        (
            format!("file://{}", upstream.display()),
            format!("file://{}", fork.display()),
        )
    }

    #[test]
    fn a_fork_clones_as_origin_with_upstream_added_and_its_branches_tracked() {
        if Command::new("git").arg("--version").output().is_err() {
            eprintln!("skipped: no git");
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let (upstream, fork) = upstream_and_fork(root.path());
        let folder = root.path().join("ws/projects/acme/widget");
        let folder_text = folder.to_string_lossy().into_owned();
        let branches = vec!["fix-retry".to_owned(), "never-pushed".to_owned()];
        let job = Job {
            url: &fork,
            folder: &folder_text,
            token: None,
            github: false,
            fork: Some(ForkSetup {
                upstream_url: &upstream,
                default_branch: "main",
                branches: &branches,
            }),
        };
        let (out, code) = run(&script(&job), root.path());
        assert_eq!(outcome(&out, code, false), Outcome::Cloned, "{out}");
        assert_eq!(git_out(&folder, &["remote", "get-url", "origin"]), fork);
        assert_eq!(
            git_out(&folder, &["remote", "get-url", "upstream"]),
            upstream
        );
        assert_eq!(
            git_out(&folder, &["rev-parse", "--abbrev-ref", "main@{upstream}"]),
            "upstream/main"
        );
        assert_eq!(
            git_out(&folder, &["log", "-1", "--format=%s", "main"]),
            "second, upstream only"
        );
        assert_eq!(
            git_out(
                &folder,
                &["rev-parse", "--abbrev-ref", "fix-retry@{upstream}"]
            ),
            "origin/fix-retry"
        );
        assert_eq!(
            git_out(&folder, &["branch", "--list", "never-pushed"]),
            "",
            "a branch the fork lacks is left out"
        );
    }

    #[test]
    fn a_checkout_already_there_gets_the_fork_and_its_branches() {
        if Command::new("git").arg("--version").output().is_err() {
            eprintln!("skipped: no git");
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let (upstream, fork) = upstream_and_fork(root.path());
        let folder = root.path().join("code/widget");
        git(
            root.path(),
            &["clone", "-q", &upstream, &folder.to_string_lossy()],
        );
        let folder_text = folder.to_string_lossy().into_owned();
        let branches = vec!["fix-retry".to_owned()];
        let add = [NewRemote {
            name: "fork".into(),
            url: fork.clone(),
        }];
        let link = Link {
            folder: &folder_text,
            token: None,
            github: false,
            add: &add,
            fork_remote: "fork",
            upstream_remote: "origin",
            branches: &branches,
        };
        let (out, code) = run(&link_script(&link), root.path());
        assert!(linked(&out, code), "{out}");
        assert_eq!(git_out(&folder, &["remote", "get-url", "origin"]), upstream);
        assert_eq!(git_out(&folder, &["remote", "get-url", "fork"]), fork);
        assert_eq!(
            git_out(
                &folder,
                &["rev-parse", "--abbrev-ref", "fix-retry@{upstream}"]
            ),
            "fork/fix-retry"
        );
        // Run again, the remote is there: it fails without harm.
        let (out, code) = run(&link_script(&link), root.path());
        assert!(!linked(&out, code), "{out}");
    }

    #[test]
    fn clones_then_reuses_and_refuses_other_files() {
        if Command::new("git").arg("--version").output().is_err() {
            eprintln!("skipped: no git");
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("src");
        std::fs::create_dir(&src).unwrap();
        git(&src, &["init", "-q", "-b", "main"]);
        git(
            &src,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.com",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "first",
            ],
        );
        let url = format!("file://{}", src.display());
        let folder = root.path().join("ws/projects/org/repo");
        let folder_text = folder.to_string_lossy().into_owned();
        let job = Job {
            url: &url,
            folder: &folder_text,
            token: None,
            github: true,
            fork: None,
        };
        let (out, code) = run(&script(&job), root.path());
        assert_eq!(outcome(&out, code, false), Outcome::Cloned, "{out}");
        assert!(folder.join(".git").is_dir());
        assert!(!root.path().join("ws/projects/org/.repo.importing").exists());

        let (out, code) = run(&script(&job), root.path());
        assert_eq!(outcome(&out, code, false), Outcome::Existing(url.clone()));

        let other = root.path().join("ws/projects/org/other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("notes.txt"), "mine").unwrap();
        let other_text = other.to_string_lossy().into_owned();
        let job = Job {
            folder: &other_text,
            ..job
        };
        let (out, code) = run(&script(&job), root.path());
        assert_eq!(outcome(&out, code, false), Outcome::Occupied);

        let missing = format!("file://{}/nope", root.path().display());
        let gone = root.path().join("ws/projects/org/gone");
        let gone_text = gone.to_string_lossy().into_owned();
        let job = Job {
            url: &missing,
            folder: &gone_text,
            ..job
        };
        let (out, code) = run(&script(&job), root.path());
        let Outcome::Failed(said) = outcome(&out, code, false) else {
            panic!("a missing repository fails: {out}");
        };
        assert!(
            said.contains("does not appear to be a git repository"),
            "{said}"
        );
        assert!(!gone.exists());
        assert!(!root.path().join("ws/projects/org/.gone.importing").exists());
    }
}
