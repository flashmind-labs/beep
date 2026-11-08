use super::*;
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git runs");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Runs the script as a node's shell would: its code and what it printed.
fn sh(script: &str) -> (Option<i32>, String) {
    let out = Command::new("sh")
        .arg("-c")
        .arg(script)
        .output()
        .expect("sh runs");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

fn path(folder: &Path) -> &str {
    folder.to_str().expect("a UTF-8 path")
}

fn state(folder: &Path) -> CheckoutState {
    let (code, out) = sh(&state_script(path(folder)));
    assert_eq!(code, Some(0));
    parse_state(&out).expect("state parses")
}

fn run(folder: &Path, branch: &str, which: Move) -> Option<i32> {
    sh(&move_script(path(folder), branch, which)).0
}

fn commit(dir: &Path, file: &str) {
    fs::write(dir.join(file), file).expect("a file");
    git(dir, &["add", file]);
    git(dir, &["commit", "-m", file]);
}

#[test]
fn pull_and_push_move_only_when_git_can_do_it_cleanly() {
    let tmp = TempDir::new().expect("a temp dir");
    let root = tmp.path();
    git(root, &["init", "--bare", "-b", "main", "origin.git"]);
    let (one, two) = (root.join("one"), root.join("two"));
    git(root, &["clone", "origin.git", "one"]);
    git(&one, &["checkout", "-b", "main"]);
    commit(&one, "a.txt");
    git(&one, &["push", "-u", "origin", "main"]);
    git(root, &["clone", "origin.git", "two"]);

    let fresh = state(&two);
    assert_eq!(fresh.branch.as_deref(), Some("main"));
    assert_eq!(fresh.upstream.as_deref(), Some("origin/main"));
    assert_eq!((fresh.ahead, fresh.behind), (0, 0));

    // A fast-forward pull; the state sees the other clone's commit through its fetch.
    commit(&one, "b.txt");
    git(&one, &["push"]);
    let fetch_head = two.join(".git/FETCH_HEAD");
    fs::write(&fetch_head, "another fetch owns this file\n").expect("write FETCH_HEAD");
    assert_eq!(state(&two).behind, 1);
    assert_eq!(run(&two, "main", Move::Pull), Some(0));
    assert_eq!(
        fs::read_to_string(fetch_head).expect("read FETCH_HEAD"),
        "another fetch owns this file\n"
    );
    assert!(two.join("b.txt").exists());
    assert_eq!(state(&two).behind, 0);

    // Diverged: the pull is refused and nothing changes.
    commit(&one, "c.txt");
    git(&one, &["push"]);
    commit(&two, "d.txt");
    let head = git_out(&two, &["rev-parse", "HEAD"]);
    let diverged = state(&two);
    assert_eq!((diverged.ahead, diverged.behind), (1, 1));
    assert!(run(&two, "main", Move::Pull).is_some_and(|code| code != 0));
    assert_eq!(git_out(&two, &["rev-parse", "HEAD"]), head);
    assert!(!two.join("c.txt").exists());

    // Behind the remote, a push is rejected and never forced.
    assert!(run(&two, "main", Move::Push).is_some_and(|code| code != 0));
    let remote = root.join("origin.git");
    assert!(!git_out(&remote, &["log", "--format=%s", "main"]).contains("d.txt"));

    // A first push sets the upstream.
    git(&two, &["checkout", "-b", "feature/x"]);
    commit(&two, "e.txt");
    assert_eq!(state(&two).upstream, None);
    assert_eq!(run(&two, "feature/x", Move::Push), Some(0));
    let pushed = state(&two);
    assert_eq!(pushed.upstream.as_deref(), Some("origin/feature/x"));
    assert_eq!((pushed.ahead, pushed.behind), (0, 0));

    // A stale branch name changes nothing, and a detached HEAD is named.
    assert_eq!(run(&two, "main", Move::Push), Some(10));
    git(&two, &["checkout", "--detach"]);
    assert_eq!(state(&two).branch, None);
    assert_eq!(run(&two, "main", Move::Pull), Some(9));
    assert_eq!(
        sh(&move_script("/no/such/folder", "main", Move::Pull)).0,
        Some(3)
    );
}

#[test]
fn a_failed_sign_in_points_at_settings() {
    let why = move_failure(
        Some(128),
        "fatal: Authentication failed for 'https://host/r.git/'",
    );
    assert!(why.contains("Settings"));
    assert_eq!(
        move_failure(Some(128), "fatal: Not possible to fast-forward, aborting."),
        "fatal: Not possible to fast-forward, aborting."
    );
}

#[test]
fn the_scripts_quote_what_they_are_given() {
    assert!(move_script("/srv/my project", "-x", Move::Push).contains("b=-x;"));
    assert!(state_script("/srv/my project").contains("'/srv/my project'"));
}
