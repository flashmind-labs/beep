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
    assert!(text.contains(&format!(
        "BEEP_GIT_TOKEN={} git -c credential.helper= -c",
        quote("tok_abc'123")
    )));
    assert!(text.contains(&format!("u={}", quote("https://git.example.com/o/r.git"))));
    assert!(text.contains(&format!("t={}", quote("/w/projects/o/.r.importing"))));
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

use crate::ShellOutput;
use async_trait::async_trait;
use std::sync::Mutex;

/// Records the scripts it is given and answers each with a fixed output.
struct FakeShell {
    said: ShellOutput,
    scripts: Mutex<Vec<String>>,
}

impl FakeShell {
    fn prints(output: &str, exit_code: i32) -> FakeShell {
        FakeShell {
            said: ShellOutput::exited(exit_code, output),
            scripts: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl Shell for FakeShell {
    async fn run(&self, script: &str, _: Duration) -> anyhow::Result<ShellOutput> {
        self.scripts
            .lock()
            .expect("the log is not poisoned")
            .push(script.to_owned());
        Ok(self.said.clone())
    }
}

fn widget() -> RepoRef {
    RepoRef::parse("acme/widget", &[]).expect("a repository")
}

#[tokio::test]
async fn an_import_clones_through_a_fake_shell() {
    let shell = FakeShell::prints("\nBEEP_IMPORT: cloned\n", 0);
    let token = SecretString::from("tok_abc");
    let cloned = run_clone(
        &shell,
        "build-box",
        &widget(),
        None,
        "/srv/projects/acme/widget",
        Some(&token),
    )
    .await;
    assert_eq!(cloned, Ok(true));
    let scripts = shell.scripts.lock().expect("the log is not poisoned");
    assert_eq!(scripts.len(), 1);
    assert!(scripts[0].contains(&format!(
        "u={}",
        quote("https://github.com/acme/widget.git")
    )));
    assert!(scripts[0].contains(&format!("d={}", quote("/srv/projects/acme/widget"))));
}

#[tokio::test]
async fn a_checkout_of_the_same_repository_is_kept_and_another_refused() {
    let same = FakeShell::prints(
        "\nBEEP_IMPORT: existing git@github.com:acme/widget.git\n",
        0,
    );
    let kept = run_clone(&same, "build-box", &widget(), None, "/srv/w", None).await;
    assert_eq!(kept, Ok(false));
    let other = FakeShell::prints("\nBEEP_IMPORT: existing https://github.com/acme/other\n", 0);
    let refused = run_clone(&other, "build-box", &widget(), None, "/srv/w", None).await;
    assert!(
        matches!(refused, Err(ImportError::OtherRepo { .. })),
        "{refused:?}"
    );
}

#[tokio::test]
async fn a_refused_clone_says_why() {
    let shell = FakeShell::prints("fatal: repository not found", 128);
    let refused = run_clone(&shell, "build-box", &widget(), None, "/srv/w", None).await;
    assert!(
        matches!(refused, Err(ImportError::NotFound { .. })),
        "{refused:?}"
    );
}
