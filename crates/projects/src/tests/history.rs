use super::*;
use std::{path::Path, process::Command};

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("UTF-8 output")
}
fn script(script: &str) -> BTreeMap<String, String> {
    let out = Command::new("sh")
        .args(["-c", script])
        .output()
        .expect("shell runs");
    assert!(
        out.status.success(),
        "script failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    decode(&String::from_utf8(out.stdout).expect("UTF-8 framing")).expect("valid framing")
}
#[test]
fn history_includes_root_merges_other_branches_and_worktrees() {
    let temp = tempfile::tempdir().expect("fixture");
    let dir = temp.path();
    git(dir, &["init", "-b", "main"]);
    std::fs::write(dir.join("first.txt"), "first\n").expect("file");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "Initial"]);
    let initial = CommitId::from_str(git(dir, &["rev-parse", "HEAD"]).trim()).expect("SHA");
    let root_patch = script(&patch_context_script(
        dir.to_str().expect("path"),
        &initial,
        &[],
        3,
    ));
    assert!(root_patch["patch"].contains("+first"));
    git(dir, &["checkout", "-b", "feature"]);
    std::fs::write(dir.join("feature.txt"), "feature\n").expect("file");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "Feature"]);
    git(dir, &["checkout", "main"]);
    git(dir, &["merge", "--no-ff", "feature", "-m", "Merge feature"]);
    git(dir, &["worktree", "add", "-b", "work", "worktree"]);
    let ask = HistoryAsk {
        folder: dir.to_str().expect("path"),
        revision: None,
        offset: 0,
        search: "",
    };
    let history = parse_history(script(&history_script(&ask)), 0).expect("history");
    assert_eq!(history.commits.len(), 3);
    assert_eq!(history.commits[0].parents.len(), 2);
    assert!(history.commits.last().expect("root").parents.is_empty());
    assert_eq!(history.worktrees.len(), 2);
    assert!(history.worktrees[0].main);
    assert_eq!(history.worktrees[1].branch.as_deref(), Some("work"));
    assert_eq!(history.refs.len(), 3);
    assert!(history.next.is_none());
    let merge = &history.commits[0].id;
    assert!(script(&patch_context_script(ask.folder, merge, &[], 3))["patch"].contains("+feature"));
    let files = parse_files(&script(&files_script(ask.folder, merge))["files"]).expect("files");
    assert_eq!(files[0].path, "feature.txt");
    assert_eq!(files[0].status, "added");
    let filtered = HistoryAsk {
        search: "Feature",
        ..ask
    };
    assert_eq!(
        parse_history(script(&history_script(&filtered)), 0)
            .expect("filtered")
            .commits
            .len(),
        2
    );
}
#[test]
fn pagination_does_not_cap_the_complete_history() {
    let temp = tempfile::tempdir().expect("fixture");
    let dir = temp.path();
    git(dir, &["init", "-b", "main"]);

    for _ in 0..53 {
        git(dir, &["commit", "--allow-empty", "-m", "Work"]);
    }

    let ask = HistoryAsk {
        folder: dir.to_str().expect("path"),
        revision: None,
        offset: 0,
        search: "",
    };
    let first = parse_history(script(&history_script(&ask)), 0).expect("page");
    assert_eq!(first.commits.len(), PAGE_SIZE);
    assert_eq!(first.next, Some(50));
    let ask = HistoryAsk { offset: 50, ..ask };
    let last = parse_history(script(&history_script(&ask)), 50).expect("page");
    assert_eq!(last.commits.len(), 3);
    assert!(last.next.is_none());
    assert_ne!(first.commits[49].id, last.commits[0].id);
}
#[test]
fn empty_repositories_and_unusual_filenames_are_read() {
    let temp = tempfile::tempdir().expect("fixture");
    let dir = temp.path();
    git(dir, &["init", "-b", "main"]);
    let ask = HistoryAsk {
        folder: dir.to_str().expect("path"),
        revision: None,
        offset: 0,
        search: "",
    };
    assert!(
        parse_history(script(&history_script(&ask)), 0)
            .expect("empty")
            .commits
            .is_empty()
    );
    let name = "name with\nnewline.txt";
    std::fs::write(dir.join(name), "content\n").expect("file");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "File"]);
    let id = CommitId::from_str(git(dir, &["rev-parse", "HEAD"]).trim()).expect("SHA");
    let files = parse_files(&script(&files_script(ask.folder, &id))["files"]).expect("files");
    assert_eq!(files[0].path, name);
    assert!(
        script(&patch_context_script(
            ask.folder,
            &id,
            &[name.to_owned()],
            3
        ))["patch"]
            .contains("+content")
    );
}
#[test]
fn rejects_clipped_output_and_non_commit_identifiers() {
    assert!(decode("@@patch 12\nYWJj\n@@end\n").is_err());
    assert!(decode("@@patch 3\nYWJj\n").is_err());
    assert!(CommitId::from_str("--all").is_err());
    assert!(CommitId::from_str("HEAD; echo no").is_err());
    assert!(CommitId::from_str(&"a".repeat(40)).is_ok());
    let files = parse_files("R100\0old name\0new name\0").expect("rename");
    assert_eq!(files[0].old_path.as_deref(), Some("old name"));
}
