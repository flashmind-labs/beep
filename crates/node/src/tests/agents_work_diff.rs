use super::*;
use crate::agents::checkout::tests::{has_git, run as run_git};
use crate::agents::work_diff::parse_git_patch;
use beep_proto::{DiffLeftOut, DiffStatus};
use tempfile::TempDir;

fn ask(base: &str, head: &str) -> DiffAsk {
    DiffAsk {
        root: String::new(),
        base: base.to_owned(),
        head: head.to_owned(),
        only: Vec::new(),
        lockfiles: false,
    }
}

/// A checkout on `main` with two files committed, and a branch `work`
/// off it.
fn checkout() -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("a temp folder");
    let root = paths::canonicalize(dir.path()).expect("it resolves");
    run_git(&root, &["init", "-q", "-b", "main"]);
    fs::write(root.join("keep.txt"), "one\ntwo\nthree\n").unwrap();
    fs::write(root.join("old.txt"), "moved\nalong\nwith\nus\n").unwrap();
    fs::write(root.join("gone.txt"), "bye\n").unwrap();
    run_git(&root, &["add", "."]);
    run_git(&root, &["commit", "-q", "-m", "first"]);
    (dir, root)
}

fn file<'a>(diff: &'a WorkDiff, path: &str) -> &'a beep_proto::DiffFile {
    diff.files
        .iter()
        .find(|f| f.path == path)
        .unwrap_or_else(|| {
            panic!(
                "{path} in {:?}",
                diff.files.iter().map(|f| &f.path).collect::<Vec<_>>()
            )
        })
}

#[test]
fn reads_commits_uncommitted_and_untracked_changes() {
    if !has_git() {
        return;
    }
    let (_dir, root) = checkout();
    run_git(&root, &["checkout", "-q", "-b", "work"]);
    fs::write(root.join("keep.txt"), "one\n2\nthree\n").unwrap();
    run_git(&root, &["commit", "-q", "-am", "change"]);
    // Uncommitted: a rename, a deletion, a new untracked file and a
    // binary one.
    run_git(&root, &["mv", "old.txt", "new.txt"]);
    run_git(&root, &["rm", "-q", "gone.txt"]);
    fs::write(root.join("fresh.txt"), "hello\nworld").unwrap();
    fs::write(root.join("blob.bin"), [0u8, 1, 2, 3]).unwrap();
    fs::write(root.join("Cargo.lock"), "a\nb\n").unwrap();
    let diff = read(&root, &ask("main", "")).expect("the diff reads");
    assert_eq!(diff.base, "main");
    assert!(diff.working_tree);
    assert_eq!(diff.root, root.to_string_lossy());
    let keep = file(&diff, "keep.txt");
    assert_eq!(keep.status, DiffStatus::Modified {});
    assert_eq!((keep.additions, keep.deletions), (1, 1));
    let moved = file(&diff, "new.txt");
    assert_eq!(moved.status, DiffStatus::Renamed {});
    assert_eq!(moved.old_path.as_deref(), Some("old.txt"));
    assert_eq!(file(&diff, "gone.txt").status, DiffStatus::Deleted {});
    let fresh = file(&diff, "fresh.txt");
    assert_eq!(fresh.status, DiffStatus::Added {});
    assert_eq!(fresh.additions, 2);
    assert!(fresh.hunks[0].lines[1].no_eol);
    assert_eq!(file(&diff, "blob.bin").status, DiffStatus::Binary {});
    assert_eq!(
        file(&diff, "Cargo.lock").left_out,
        Some(DiffLeftOut::Lockfile {})
    );
}

#[test]
fn base_at_gives_only_what_is_uncommitted_even_with_an_origin_head() {
    if !has_git() {
        return;
    }
    let (_dir, root) = checkout();
    run_git(&root, &["update-ref", "refs/remotes/origin/main", "main"]);
    run_git(
        &root,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    run_git(&root, &["checkout", "-q", "-b", "work"]);
    fs::write(root.join("keep.txt"), "one\n2\nthree\n").unwrap();
    run_git(&root, &["commit", "-q", "-am", "change"]);
    fs::write(root.join("gone.txt"), "still\n").unwrap();
    fs::write(root.join("fresh.txt"), "hello\n").unwrap();
    let diff = read(&root, &ask("@", "")).expect("the diff reads");
    let mut paths: Vec<_> = diff.files.iter().map(|f| f.path.as_str()).collect();
    paths.sort_unstable();
    assert_eq!(paths, ["fresh.txt", "gone.txt"]);
    assert!(diff.working_tree);
}

#[test]
fn a_branch_with_a_worktree_inside_gives_its_working_tree() {
    if !has_git() {
        return;
    }
    let (_dir, root) = checkout();
    run_git(
        &root,
        &["worktree", "add", "-q", "-b", "wt", ".worktrees/wt"],
    );
    let wt = root.join(".worktrees/wt");
    fs::write(wt.join("note.md"), "new\n").unwrap();
    let diff = read(&root, &ask("main", "wt")).expect("the diff reads");
    assert!(diff.working_tree);
    assert_eq!(diff.root, wt.to_string_lossy());
    assert_eq!(diff.files.len(), 1);
    assert_eq!(diff.files[0].path, "note.md");
}

#[test]
fn the_default_base_is_found_and_a_subfolder_stays_inside() {
    if !has_git() {
        return;
    }
    let (_dir, root) = checkout();
    fs::create_dir(root.join("sub")).unwrap();
    fs::write(root.join("sub/in.txt"), "x\n").unwrap();
    fs::write(root.join("keep.txt"), "changed\n").unwrap();
    let diff = read(&root.join("sub"), &ask("", "")).expect("the diff reads");
    assert_eq!(diff.base, "main");
    let paths: Vec<_> = diff.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["in.txt"]);
}

#[test]
fn refuses_bad_names_and_folders_outside_a_checkout() {
    if !has_git() {
        return;
    }
    let (_dir, root) = checkout();
    for bad in ["--output=/tmp/x", "a..b", "a b"] {
        let e = read(&root, &ask(bad, "")).unwrap_err();
        assert!(e.to_string().contains("not a branch name"), "{bad}: {e:#}");
    }
    assert!(read(&root, &ask("nope", "")).is_err());
    let plain = TempDir::new().unwrap();
    let e = read(plain.path(), &ask("", "")).unwrap_err();
    assert!(e.to_string().contains("not in a git checkout"), "{e:#}");
}

#[test]
fn a_checkout_with_no_commits_shows_its_files() {
    if !has_git() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let root = paths::canonicalize(dir.path()).unwrap();
    run_git(&root, &["init", "-q", "-b", "main"]);
    fs::write(root.join("a.txt"), "a\n").unwrap();
    let diff = read(&root, &ask("", "")).expect("the diff reads");
    assert_eq!(diff.base, "HEAD");
    assert_eq!(diff.merge_base, "");
    assert_eq!(diff.files.len(), 1);
    assert_eq!(diff.files[0].status, DiffStatus::Added {});
}
#[test]
fn historical_patch_uses_its_recorded_text_without_working_tree_changes() {
    let (_temp, root) = checkout();
    let head = git(&root, &["rev-parse", "HEAD"]).unwrap();
    let patch = git(
        &root,
        &["show", "--format=", "--root", "--no-ext-diff", head.trim()],
    )
    .unwrap();
    fs::write(root.join("keep.txt"), "uncommitted\n").unwrap();
    let diff = parse_git_patch(&ask("", head.trim()), &patch);
    assert!(!diff.working_tree);
    assert_eq!(diff.files.len(), 3);
    assert_eq!(file(&diff, "keep.txt").additions, 3);
    assert!(
        !file(&diff, "keep.txt")
            .hunks
            .iter()
            .flat_map(|h| &h.lines)
            .any(|line| line.text == "uncommitted")
    );
}
