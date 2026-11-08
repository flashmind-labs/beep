use super::*;
use std::{fs, process::Command};

fn run(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .status()
        .expect("git runs")
        .success();
    assert!(ok, "git {args:?}");
}

const REVIEWER: &str =
    "---\nname: reviewer\ndescription: Reviews\n---\nCheck every SQL query for injection.\n";

#[test]
fn a_reviewer_counts_only_as_the_base_has_it() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    run(root, &["init", "-q", "-b", "main"]);
    fs::create_dir_all(root.join(".claude/agents")).expect("folder");
    fs::write(root.join("README"), "hi").expect("file");
    run(root, &["add", "."]);
    run(root, &["commit", "-qm", "start"]);
    fs::write(root.join(".claude/agents/reviewer.md"), REVIEWER).expect("file");
    assert_eq!(
        find(root, "HEAD", "claude"),
        None,
        "a reviewer the changes add is not used"
    );
    run(root, &["add", "."]);
    run(root, &["commit", "-qm", "reviewer"]);
    let found = find(root, "HEAD", "codex").expect("the committed reviewer");
    assert_eq!(found.path, ".claude/agents/reviewer.md");
    assert_eq!(found.prompt, "Check every SQL query for injection.");
    fs::write(
        root.join(".claude/agents/reviewer.md"),
        REVIEWER.replace("Check", "Approve"),
    )
    .expect("file");
    assert_eq!(
        find(root, "HEAD", "claude"),
        None,
        "a reviewer the changes edit is not used"
    );
    assert_eq!(find(root, "--output=x", "claude"), None);
}
