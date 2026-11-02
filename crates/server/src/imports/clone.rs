//! The shell script that clones a repository on a node, and what its
//! output means.
//!
//! The clone goes to a hidden folder beside the checkout and is renamed
//! into place once complete, so an interrupted clone never looks like a
//! checkout. A token travels as a shell variable git's credential helper
//! reads: never in a command line or in the saved remote URL.

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
}

/// Quotes `text` for sh.
pub(super) fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The git helper that answers with `$BEEP_GIT_TOKEN`, and nothing when it
/// is empty, so git goes on without credentials.
const HELPER: &str = r#"!f() { test "$1" = get && test -n "$BEEP_GIT_TOKEN" || return 0; echo username=x-access-token; echo "password=$BEEP_GIT_TOKEN"; }; f"#;

/// The script. Secret: it holds the token.
pub(crate) fn script(job: &Job<'_>) -> SecretString {
    let (parent, leaf) = split(job.folder);
    let temp = format!("{parent}/.{leaf}.importing");
    // With a token from Beep, only its helper answers, so a keychain
    // helper neither shadows it nor stores it. Without one, the user's own
    // helpers stay, with `$GH_TOKEN` after them on GitHub.
    let git = match (job.token, job.github) {
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
         elif mkdir -p \"$p\" && rm -rf \"$t\" && {git} clone --quiet -- \"$u\" \"$t\"; then\n\
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
    fn the_output_says_what_happened() {
        assert_eq!(
            outcome("noise\n\nBEEP_IMPORT: cloned\n", Some(0), false),
            Outcome::Cloned
        );
        assert_eq!(
            outcome(
                "BEEP_IMPORT: existing git@github.com:o/r.git",
                Some(0),
                false
            ),
            Outcome::Existing("git@github.com:o/r.git".into())
        );
        assert_eq!(
            outcome("BEEP_IMPORT: existing ", Some(0), false),
            Outcome::Existing(String::new())
        );
        assert_eq!(
            outcome("BEEP_IMPORT: occupied", Some(0), false),
            Outcome::Occupied
        );
        assert_eq!(
            outcome("BEEP_IMPORT: nogit", Some(0), false),
            Outcome::NoGit
        );
        assert_eq!(
            outcome("fatal: boom", Some(128), false),
            Outcome::Failed("fatal: boom".into())
        );
        assert_eq!(outcome("", None, true), Outcome::TimedOut);
    }

    fn failure(output: &str, had_token: bool) -> String {
        explain(Failure {
            output,
            project: "org/repo".into(),
            host: "GitHub".into(),
            machine: "build-box",
            had_token,
        })
        .to_string()
    }

    #[test]
    fn git_errors_read_as_plain_sentences() {
        let missing = "remote: Repository not found.\nfatal: repository 'https://github.com/org/repo.git/' not found";
        assert_eq!(
            failure(missing, false),
            "Could not find org/repo on GitHub. If it is private, add a token for GitHub in \
             Settings > Credentials."
        );
        assert_eq!(
            failure(missing, true),
            "Could not find org/repo on GitHub, or your token there has no access to it. \
             Check it in Settings > Credentials."
        );
        assert_eq!(
            failure(
                "fatal: could not read Username for 'https://github.com': terminal prompts disabled",
                false
            ),
            failure(missing, false)
        );
        assert_eq!(
            failure(
                "fatal: unable to access 'https://github.com/org/repo.git/': Could not resolve host: github.com",
                false
            ),
            "build-box could not reach GitHub."
        );
        assert_eq!(
            failure("fatal: write error: No space left on device", false),
            "build-box is out of disk space."
        );
        assert_eq!(
            failure("warning: x\nfatal: something odd\n", false),
            "The clone failed on build-box: something odd"
        );
    }

    #[test]
    fn the_token_stays_out_of_the_url_and_command_lines() {
        let token = SecretString::from("tok_abc'123");
        let script = script(&Job {
            url: "https://git.example.com/o/r.git",
            folder: "/w/projects/o/r",
            token: Some(&token),
            github: false,
        });
        let text = script.expose_secret();
        assert!(text.contains(r"BEEP_GIT_TOKEN='tok_abc'\''123' git -c credential.helper= -c"));
        assert!(text.contains("u='https://git.example.com/o/r.git'"));
        assert!(text.contains("t='/w/projects/o/.r.importing'"));
    }

    /// The script against a real git, as a node's shell runs it: `exec
    /// 2>&1`, then the script through `eval`.
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
