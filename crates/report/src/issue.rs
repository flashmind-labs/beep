use crate::bundle::{MachineEntry, Manifest};
use crate::collect::Answer;
use crate::redact::{RedactError, redact};
use std::env;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

pub const REPO: &str = "flashmind-labs/beep";
pub const LABEL: &str = "report";

const SHORT_ID: usize = 8;

/// The public issue: the user's text, the reference and versions. Never logs,
/// machine names, paths or ids beyond the sender's short one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub title: String,
    pub body: String,
}

#[derive(Debug, Error)]
pub enum IssueError {
    #[error("could not run gh: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("gh failed: {0}")]
    Failed(String),
    #[error("gh printed no issue URL")]
    NoUrl,
}

/// Redacts `manifest.text` itself and fails closed if it cannot.
pub fn issue(manifest: &Manifest, reference: &str) -> Result<Issue, RedactError> {
    let text = quote_mentions(&redact(&manifest.text)?);
    let mut body = String::new();
    for line in text.lines() {
        body.push_str("> ");
        body.push_str(line);
        body.push('\n');
    }
    if body.is_empty() {
        body.push_str(">\n");
    }
    let sender = manifest.sender.to_string();
    let short: String = sender.chars().take(SHORT_ID).collect();
    let _ = writeln!(body, "\nReference: {reference}");
    let _ = writeln!(body, "Device: {short}");
    let _ = writeln!(
        body,
        "App: {} {}",
        manifest.app,
        or_unknown(&manifest.app_version)
    );
    if !manifest.machines.is_empty() {
        body.push_str("\nMachines:\n");
        for machine in &manifest.machines {
            let _ = writeln!(body, "- {}", machine_line(machine));
        }
    }
    Ok(Issue {
        title: format!("Problem report {reference}"),
        body,
    })
}

/// Wraps every `@word` in backticks so the issue pings nobody.
fn quote_mentions(text: &str) -> String {
    let word = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '/');
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('@') {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 1..];
        let len = tail.find(|c| !word(c)).unwrap_or(tail.len());
        if len == 0 {
            out.push('@');
        } else {
            out.push('`');
            out.push('@');
            out.push_str(&tail[..len]);
            out.push('`');
        }
        rest = &tail[len..];
    }
    out.push_str(rest);
    out
}

fn machine_line(m: &MachineEntry) -> String {
    format!(
        "{}, {} {}, {}: {}",
        m.kind,
        m.os,
        m.os_version,
        or_unknown(&m.version),
        answer_word(&m.answer)
    )
}

fn or_unknown(version: &str) -> &str {
    if version.is_empty() {
        "unknown"
    } else {
        version
    }
}

/// Never the reason text of Offline and Failed: it can hold names or paths.
fn answer_word(answer: &Answer) -> &'static str {
    match answer {
        Answer::Answered => "answered",
        Answer::OlderNode => "older node",
        Answer::Silent => "did not answer",
        Answer::Offline { .. } => "offline",
        Answer::Failed { .. } => "failed",
    }
}

/// `gh` (`gh.exe` on Windows) in a `PATH` folder.
pub fn find_gh() -> Option<PathBuf> {
    let name = if cfg!(windows) { "gh.exe" } else { "gh" };
    env::split_paths(&env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

pub async fn gh_signed_in(gh: &Path) -> bool {
    Command::new(gh)
        .args(["auth", "status", "--hostname", "github.com"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .status()
        .await
        .is_ok_and(|status| status.success())
}

/// Opens the issue with the user's own `gh` and returns its URL. Runs again
/// without the label when the repository has none.
pub async fn open_issue(gh: &Path, issue: &Issue) -> Result<String, IssueError> {
    match create(gh, issue, true).await {
        Err(IssueError::Failed(stderr)) if label_missing(&stderr) => create(gh, issue, false).await,
        other => other,
    }
}

fn label_missing(stderr: &str) -> bool {
    let stderr = stderr.to_lowercase();
    stderr.contains("not found") && stderr.contains("label")
}

async fn create(gh: &Path, issue: &Issue, label: bool) -> Result<String, IssueError> {
    let mut command = Command::new(gh);
    command
        .args(["issue", "create", "-R", REPO, "--title", &issue.title])
        .args(["--body-file", "-"]);
    if label {
        command.args(["--label", LABEL]);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        // A gh that exits early closes the pipe; its exit status says why.
        let _ = stdin.write_all(issue.body.as_bytes()).await;
    }
    let output = child.wait_with_output().await?;
    if !output.status.success() {
        return Err(IssueError::Failed(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| line.starts_with("https://"))
        .map(str::to_string)
        .ok_or(IssueError::NoUrl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::period::Period;
    use beep_mesh::DeviceKind;
    use iroh::SecretKey;

    fn machine(name: &str, answer: Answer) -> MachineEntry {
        MachineEntry {
            id: SecretKey::generate().public(),
            name: name.into(),
            kind: DeviceKind::Node,
            answer,
            version: "0.4.1".into(),
            os: "linux".into(),
            os_version: "6.8".into(),
        }
    }

    fn manifest(text: &str) -> Manifest {
        Manifest {
            version: 1,
            sender: SecretKey::generate().public(),
            mesh: None,
            app: "desktop".into(),
            app_version: "0.2.0".into(),
            text: text.into(),
            period: Period::Today,
            since: 0,
            made_at: 0,
            machines: vec![
                machine("build-box", Answer::Answered),
                machine(
                    "gpu-box",
                    Answer::Failed {
                        reason: "/path/to/work".into(),
                    },
                ),
                machine("old-box", Answer::Silent),
            ],
            files: Vec::new(),
        }
    }

    #[test]
    fn the_issue_names_no_machine_path_or_log() {
        let m = manifest("sync stalls\nsecond line cc @someone");
        let got = issue(&m, "R-1234").expect("redaction rules build");
        let sender = m.sender.to_string();
        assert_eq!(got.title, "Problem report R-1234");
        assert!(got.body.contains("R-1234"));
        assert!(got.body.contains("node, linux 6.8, 0.4.1: answered"));
        assert!(got.body.contains("did not answer"));
        assert!(got.body.contains("failed"));
        assert!(got.body.contains(&sender[..8]));
        assert!(got.body.contains("> sync stalls\n> second line"));
        assert!(got.body.contains("`@someone`"));
        for secret in ["build-box", "gpu-box", "old-box", "/path/to/work", &sender] {
            assert!(!got.body.contains(secret), "{secret} leaked");
        }
    }

    #[test]
    fn the_text_is_redacted_here() {
        let got = issue(&manifest("token=abcdef123456789012345678"), "R-1")
            .expect("redaction rules build");
        assert!(!got.body.contains("abcdef123456789012345678"));
    }

    #[test]
    fn an_empty_version_reads_unknown() {
        let mut m = manifest("x");
        m.machines[0].version.clear();
        let got = issue(&m, "R-1").expect("redaction rules build");
        assert!(got.body.contains("node, linux 6.8, unknown: answered"));
    }

    #[test]
    fn mentions_are_quoted_and_lone_at_signs_kept() {
        assert_eq!(quote_mentions("a @x/team, b@ c @"), "a `@x/team`, b@ c @");
    }

    #[cfg(unix)]
    mod gh {
        use super::*;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        fn stand_in(script: &str) -> (tempfile::TempDir, PathBuf) {
            let dir = tempfile::tempdir().expect("temp dir");
            let path = dir.path().join("gh");
            fs::write(&path, format!("#!/bin/sh\n{script}\n")).expect("write script");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
            (dir, path)
        }

        fn sample() -> Issue {
            Issue {
                title: "Problem report R-1".into(),
                body: "> hi\n\nReference: R-1\n".into(),
            }
        }

        #[tokio::test]
        async fn open_issue_passes_the_repo_label_and_body() {
            let (dir, gh) = stand_in(
                "echo \"$@\" > \"$(dirname \"$0\")/args\"\ncat > \"$(dirname \"$0\")/stdin\"\n\
                 echo 'Creating issue'\necho https://github.com/flashmind-labs/beep/issues/1",
            );
            let issue = sample();
            let url = open_issue(&gh, &issue).await.expect("opens");
            assert_eq!(url, "https://github.com/flashmind-labs/beep/issues/1");
            let args = fs::read_to_string(dir.path().join("args")).expect("args");
            assert!(args.contains("-R flashmind-labs/beep"), "{args}");
            assert!(args.contains("--label report"), "{args}");
            let stdin = fs::read_to_string(dir.path().join("stdin")).expect("stdin");
            assert_eq!(stdin, issue.body);
        }

        #[tokio::test]
        async fn open_issue_retries_without_a_missing_label() {
            let (_dir, gh) = stand_in(
                "cat > /dev/null\ncase \"$*\" in\n*--label*) echo \"could not add label: 'report' not found\" >&2; exit 1;;\nesac\n\
                 echo https://github.com/flashmind-labs/beep/issues/2",
            );
            let url = open_issue(&gh, &sample()).await.expect("retries");
            assert_eq!(url, "https://github.com/flashmind-labs/beep/issues/2");
        }

        #[tokio::test]
        async fn other_failures_are_not_retried() {
            let (_dir, gh) = stand_in("cat > /dev/null\necho 'auth required' >&2\nexit 1");
            let got = open_issue(&gh, &sample()).await;
            assert!(matches!(got, Err(IssueError::Failed(m)) if m == "auth required"));
        }
    }
}
