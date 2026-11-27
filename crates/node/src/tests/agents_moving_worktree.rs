use super::*;
use std::{fs, path::PathBuf};
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// A checkout with one commit on `main` and a worktree on `feat` with
/// a changed file, a staged new file, a deleted file and an untracked
/// one.
fn source() -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("a temp folder");
    let main = dir.path().join("main");
    fs::create_dir(&main).expect("a folder");
    git(&main, &["init", "-q", "--initial-branch=main"]);
    // A user's own diff settings must not change what moves.
    git(&main, &["config", "diff.noprefix", "true"]);
    fs::write(main.join("a.txt"), "one\n").expect("a file");
    fs::write(main.join("gone.txt"), "bye\n").expect("a file");
    fs::write(main.join("bin.dat"), [0u8, 1, 2, 3]).expect("a file");
    git(&main, &["add", "."]);
    git(&main, &["commit", "-q", "-m", "first"]);
    let wt = dir.path().join("wt");
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feat",
            wt.to_str().expect("utf8"),
        ],
    );
    fs::write(wt.join("a.txt"), "two\n").expect("a change");
    fs::write(wt.join("bin.dat"), [9u8, 8, 7]).expect("a change");
    fs::write(wt.join("staged.txt"), "s\n").expect("a file");
    git(&wt, &["add", "staged.txt"]);
    fs::remove_file(wt.join("gone.txt")).expect("a deletion");
    fs::write(wt.join("new.txt"), "n\n").expect("an untracked file");
    (dir, wt)
}

#[test]
fn uncommitted_work_follows_the_branch_into_a_new_worktree() {
    let (dir, wt) = source();
    let work = snapshot(&wt).expect("a snapshot");
    // The same repository stands in for the target's checkout, which
    // has the commit and, once the source lets go, the branch.
    let main = dir.path().join("main");
    let target = dir.path().join("target-wt");
    git(&wt, &["checkout", "-q", "--detach"]);
    attach(&main, &target, "feat", &work).expect("attached");
    assert_eq!(git(&target, &["symbolic-ref", "--short", "HEAD"]), "feat");
    assert_eq!(fs::read_to_string(target.join("a.txt")).unwrap(), "two\n");
    assert_eq!(fs::read(target.join("bin.dat")).unwrap(), [9u8, 8, 7]);
    assert_eq!(
        fs::read_to_string(target.join("staged.txt")).unwrap(),
        "s\n"
    );
    assert_eq!(fs::read_to_string(target.join("new.txt")).unwrap(), "n\n");
    assert!(!target.join("gone.txt").exists());
    // The source's own worktree lost nothing.
    assert_eq!(fs::read_to_string(wt.join("a.txt")).unwrap(), "two\n");
    assert!(wt.join("new.txt").exists());
}

#[test]
fn a_patch_that_does_not_apply_leaves_no_worktree_behind() {
    let (dir, wt) = source();
    let mut work = snapshot(&wt).expect("a snapshot");
    work.patch = b"diff --git a/zzz b/zzz\n--- a/zzz\n+++ b/zzz\n@@ -1 +1 @@\n-x\n+y\n".to_vec();
    let main = dir.path().join("main");
    let target = dir.path().join("target-wt");
    git(&wt, &["checkout", "-q", "--detach"]);
    let failed = attach(&main, &target, "feat", &work).expect_err("does not apply");
    assert!(format!("{failed:#}").contains("do not apply"), "{failed:#}");
    assert!(!target.exists());
    assert!(!git(&main, &["worktree", "list"]).contains("target-wt"));
}

#[test]
fn detaching_frees_the_branch_and_keeps_the_files() {
    let (dir, wt) = source();
    let main = dir.path().join("main");
    let other = dir.path().join("other");
    let at = other.to_str().expect("utf8");
    let busy = run(&main, &["worktree", "add", "--quiet", at, "feat"], None);
    assert!(busy.is_err(), "the branch is held by the worktree");
    detach(&wt).expect("detached");
    assert!(wt.join("new.txt").exists());
    assert_eq!(fs::read_to_string(wt.join("a.txt")).unwrap(), "two\n");
    run(&main, &["worktree", "add", "--quiet", at, "feat"], None).expect("the branch is free");
}

#[test]
fn a_branch_with_other_commits_here_is_refused() {
    let (dir, wt) = source();
    let main = dir.path().join("main");
    let work = snapshot(&wt).expect("a snapshot");
    git(&wt, &["checkout", "-q", "--detach"]);
    git(&main, &["checkout", "-q", "feat"]);
    fs::write(main.join("side.txt"), "x\n").expect("a file");
    git(&main, &["add", "side.txt"]);
    git(&main, &["commit", "-q", "-m", "side"]);
    git(&main, &["checkout", "-q", "main"]);
    let target = dir.path().join("target-wt");
    let failed = attach(&main, &target, "feat", &work).expect_err("diverged");
    assert!(format!("{failed:#}").contains("has commits"), "{failed:#}");
    assert!(!target.exists());
}

#[test]
fn a_branch_checked_out_elsewhere_is_refused_before_its_ref_moves() {
    let (dir, wt) = source();
    let main = dir.path().join("main");
    // The session's worktree is ahead of the branch (a commit on a
    // detached head), and here the branch is checked out in another
    // worktree.
    git(&wt, &["checkout", "-q", "--detach"]);
    git(&wt, &["commit", "-q", "-a", "-m", "ahead"]);
    let work = snapshot(&wt).expect("a snapshot");
    let other = dir.path().join("other");
    git(
        &main,
        &["worktree", "add", "-q", other.to_str().unwrap(), "feat"],
    );
    let tip = git(&main, &["rev-parse", "feat"]);
    assert_ne!(tip, work.head);
    let target = dir.path().join("target-wt");
    let failed = attach(&main, &target, "feat", &work).expect_err("checked out");
    assert!(
        format!("{failed:#}").contains("checked out in"),
        "{failed:#}"
    );
    assert_eq!(
        git(&main, &["rev-parse", "feat"]),
        tip,
        "the branch did not move under the other worktree"
    );
    assert!(!target.exists());
}

/// Puts the changes the session left behind out of a detached worktree.
fn clean(wt: &Path) {
    git(wt, &["checkout", "-q", "--force", "--detach"]);
    git(wt, &["clean", "-fdq"]);
}

/// A stash entry of the user's own, on the checkout.
fn foreign_stash(main: &Path) -> String {
    fs::write(main.join("a.txt"), "mine\n").expect("a change");
    git(main, &["stash", "push", "-q", "-m", "the user's own"]);
    git(main, &["stash", "list"])
}

#[test]
fn a_dirty_detached_worktree_is_refused_by_name_and_left_alone() {
    let (dir, wt) = source();
    let main = dir.path().join("main");
    let work = snapshot(&wt).expect("a snapshot");
    detach(&wt).expect("detached");
    let failed = check(&main, &wt, "feat").expect_err("dirty");
    assert!(
        format!("{failed:#}").contains(&wt.display().to_string()),
        "{failed:#}"
    );
    assert!(attach(&main, &wt, "feat", &work).is_err());
    assert_eq!(fs::read_to_string(wt.join("a.txt")).unwrap(), "two\n");
    assert!(run(&wt, &["symbolic-ref", "-q", "HEAD"], None).is_err());
}

#[test]
fn a_detached_head_with_a_commit_of_its_own_is_refused() {
    let (dir, wt) = source();
    let main = dir.path().join("main");
    let work = snapshot(&wt).expect("a snapshot");
    detach(&wt).expect("detached");
    clean(&wt);
    fs::write(wt.join("own.txt"), "x\n").expect("a file");
    git(&wt, &["add", "own.txt"]);
    git(&wt, &["commit", "-q", "-m", "own"]);
    let head = git(&wt, &["rev-parse", "HEAD"]);
    let failed = attach(&main, &wt, "feat", &work).expect_err("not contained");
    assert!(
        format!("{failed:#}").contains("does not contain"),
        "{failed:#}"
    );
    assert_eq!(
        git(&wt, &["rev-parse", "HEAD"]),
        head,
        "the commit is still there"
    );
}

#[test]
fn a_clean_detached_worktree_is_reused_and_the_stash_is_never_touched() {
    let (dir, wt) = source();
    let main = dir.path().join("main");
    let work = snapshot(&wt).expect("a snapshot");
    detach(&wt).expect("detached");
    clean(&wt);
    let stash = foreign_stash(&main);
    let placed = attach(&main, &wt, "feat", &work).expect("reused");
    assert!(matches!(placed, Placed::Reused { .. }));
    assert_eq!(git(&wt, &["symbolic-ref", "--short", "HEAD"]), "feat");
    assert_eq!(fs::read_to_string(wt.join("a.txt")).unwrap(), "two\n");
    assert_eq!(git(&main, &["stash", "list"]), stash);
}

#[test]
fn a_patch_that_does_not_apply_puts_a_reused_worktree_back_as_it_was() {
    let (dir, wt) = source();
    let main = dir.path().join("main");
    let mut work = snapshot(&wt).expect("a snapshot");
    work.patch = b"diff --git a/zzz b/zzz\n--- a/zzz\n+++ b/zzz\n@@ -1 +1 @@\n-x\n+y\n".to_vec();
    detach(&wt).expect("detached");
    clean(&wt);
    let at = git(&wt, &["rev-parse", "HEAD"]);
    let failed = attach(&main, &wt, "feat", &work).expect_err("does not apply");
    assert!(format!("{failed:#}").contains("do not apply"), "{failed:#}");
    assert!(
        run(&wt, &["symbolic-ref", "-q", "HEAD"], None).is_err(),
        "detached again"
    );
    assert_eq!(git(&wt, &["rev-parse", "HEAD"]), at);
    assert_eq!(git(&wt, &["status", "--porcelain"]), "");
}

#[test]
fn the_patch_is_read_with_a_cap_and_git_is_not_left_running() {
    let (_dir, wt) = source();
    // Far over a tiny cap: stopped, an error, no shorter patch.
    let failed = capped(&wt, &["diff", "HEAD", "--binary"], 3, &[]).expect_err("too large");
    assert!(format!("{failed:#}").contains("too large"), "{failed:#}");
    // A cap that fits gives all of it.
    let whole = capped(&wt, &["diff", "HEAD", "--binary"], MAX_PATCH, &[]).expect("fits");
    assert!(whole.len() > 3);
}

#[test]
fn a_repository_of_its_own_among_the_new_files_is_refused_by_name_not_dropped() {
    let (dir, wt) = source();
    let nested = wt.join("vendor/lib");
    fs::create_dir_all(&nested).expect("a folder");
    git(&nested, &["init", "-q"]);
    fs::write(nested.join("f.txt"), "x\n").expect("a file");
    let failed = snapshot(&wt).expect_err("a nested repository");
    assert!(format!("{failed:#}").contains("vendor/lib"), "{failed:#}");
    drop(dir);
}

#[test]
fn a_landed_worktree_is_cleaned_only_if_it_still_holds_what_was_sent() {
    let (dir, wt) = source();
    let _ = dir;
    let sent = snapshot(&wt).expect("a snapshot").fingerprint();
    assert!(detach_clean(&wt, Some(&sent)).expect("detached"));
    assert_eq!(git(&wt, &["status", "--porcelain"]), "");
    assert!(run(&wt, &["symbolic-ref", "-q", "HEAD"], None).is_err());

    let (_dir2, wt2) = source();
    let sent = snapshot(&wt2).expect("a snapshot").fingerprint();
    fs::write(wt2.join("a.txt"), "edited after it was sent\n").expect("an edit");
    assert!(!detach_clean(&wt2, Some(&sent)).expect("detached"));
    assert_eq!(
        fs::read_to_string(wt2.join("a.txt")).unwrap(),
        "edited after it was sent\n"
    );
    // Nothing known of what was sent (a restart): detached, left as it is.
    let (_dir3, wt3) = source();
    assert!(!detach_clean(&wt3, None).expect("detached"));
    assert!(wt3.join("new.txt").exists());
}
#[test]
fn a_move_carries_commits_and_changes_between_independent_checkouts() {
    let (dir, wt) = source();
    let main = dir.path().join("main");
    let copy = dir.path().join("copy");
    git(
        dir.path(),
        &[
            "clone",
            "--quiet",
            "--no-local",
            "--single-branch",
            "--branch",
            "main",
            main.to_str().unwrap(),
            copy.to_str().unwrap(),
        ],
    );
    fs::write(wt.join("branch.txt"), "only on the branch\n").unwrap();
    git(&wt, &["add", "branch.txt"]);
    git(
        &wt,
        &["commit", "-q", "--only", "-m", "branch work", "branch.txt"],
    );
    let work = snapshot(&wt).expect("the worktree state");
    assert!(!has_commit(&copy, &work.head), "the commit is not on main");
    let base = git(&copy, &["rev-parse", "HEAD"]);
    let bytes = bundle(&wt, &work.head, &base).expect("the commits main lacks");
    let old_head = git(&copy, &["rev-parse", "HEAD"]);
    import(&copy, &work.head, &bytes).expect("the history imports");
    assert_eq!(git(&copy, &["rev-parse", "HEAD"]), old_head);
    assert!(run(&copy, &["rev-parse", "--verify", "refs/heads/feat"], None).is_err());
    let target = copy.join(".worktrees/feat");
    let was = tip(&copy, "feat");
    let placed = attach(&copy, &target, "feat", &work).expect("the worktree moves");
    assert_eq!(git(&target, &["rev-parse", "HEAD"]), work.head);
    assert_eq!(fs::read_to_string(target.join("a.txt")).unwrap(), "two\n");
    assert!(target.join("new.txt").exists());
    assert!(!target.join("gone.txt").exists());
    assert_eq!(fs::read(target.join("bin.dat")).unwrap(), [9, 8, 7]);
    let common = git(
        &target,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    );
    // Git reports the real path, and macOS temp folders sit behind a symlink.
    assert_eq!(
        PathBuf::from(common),
        fs::canonicalize(&copy).expect("a folder").join(".git")
    );
    assert_eq!(git(&wt, &["symbolic-ref", "--short", "HEAD"]), "feat");

    // Taken back, the branch the move made goes too.
    let applied = fingerprint(&target).expect("a fingerprint");
    let moved = Moved {
        branch: "feat".into(),
        was,
        to: work.head.clone(),
    };
    take_back(&copy, &target, &placed, &applied, Some(&moved)).expect("taken back");
    assert!(!target.exists());
    assert_eq!(tip(&copy, "feat"), None);
}

#[test]
fn a_bundle_refuses_a_changed_commit() {
    let (_dir, wt) = source();
    let base = git(&wt, &["rev-parse", "main"]);
    assert!(bundle(&wt, "deadbeef", &base).is_err());
}

#[test]
fn a_worktree_already_holding_the_work_is_taken_as_it_is() {
    let (dir, wt) = source();
    let main = dir.path().join("main");
    let work = snapshot(&wt).expect("a snapshot");
    detach(&wt).expect("detached");
    let target = dir.path().join("target-wt");
    assert_eq!(attach(&main, &target, "feat", &work).expect("made"), Placed::Made);
    assert_eq!(
        attach(&main, &target, "feat", &work).expect("a retried move"),
        Placed::Held
    );
    fs::write(target.join("a.txt"), "edited since\n").expect("an edit");
    assert!(attach(&main, &target, "feat", &work).is_err(), "edits since stay the user's");
}

#[test]
fn a_bundle_holds_only_the_commits_after_its_base() {
    let (dir, wt) = source();
    let main = dir.path().join("main");
    let base = git(&main, &["rev-parse", "HEAD"]);
    fs::write(wt.join("branch.txt"), "x\n").expect("a file");
    git(&wt, &["add", "branch.txt"]);
    git(&wt, &["commit", "-q", "--only", "-m", "branch work", "branch.txt"]);
    let head = git(&wt, &["rev-parse", "HEAD"]);
    let bytes = bundle(&wt, &head, &base).expect("a bundle");
    let listed = run(&main, &["bundle", "list-heads", "-"], Some(&bytes)).expect("it reads");
    assert!(String::from_utf8_lossy(&listed).contains(&head));
    let prereqs = String::from_utf8_lossy(&bytes);
    assert!(prereqs.contains(&format!("-{base}")), "the base is a prerequisite");
}

#[test]
fn the_head_of_a_folder_outside_git_is_none() {
    let dir = TempDir::new().expect("a temp folder");
    assert_eq!(head(dir.path()).expect("read"), None);
    let (_dir, wt) = source();
    assert_eq!(head(&wt).expect("read"), Some(git(&wt, &["rev-parse", "HEAD"])));
}

#[test]
fn taking_back_removes_a_made_worktree_unchanged_since_and_leaves_one_with_edits() {
    let (dir, wt) = source();
    let main = dir.path().join("main");
    let work = snapshot(&wt).expect("a snapshot");
    detach(&wt).expect("detached");
    let target = dir.path().join("target-wt");
    let placed = attach(&main, &target, "feat", &work).expect("made");
    let applied = fingerprint(&target).expect("a fingerprint");
    take_back(&main, &target, &placed, &applied, None).expect("taken back");
    assert!(!target.exists());
    assert!(!git(&main, &["worktree", "list"]).contains("target-wt"));

    let placed = attach(&main, &target, "feat", &work).expect("made again");
    let applied = fingerprint(&target).expect("a fingerprint");
    fs::write(target.join("a.txt"), "edited since\n").expect("an edit");
    assert!(take_back(&main, &target, &placed, &applied, None).is_err());
    assert_eq!(fs::read_to_string(target.join("a.txt")).unwrap(), "edited since\n");
}

#[test]
fn taking_back_detaches_a_reused_worktree_at_its_old_commit() {
    let (dir, wt) = source();
    let main = dir.path().join("main");
    let work = snapshot(&wt).expect("a snapshot");
    detach(&wt).expect("detached");
    clean(&wt);
    let prior = git(&wt, &["rev-parse", "HEAD"]);
    let placed = attach(&main, &wt, "feat", &work).expect("reused");
    let applied = fingerprint(&wt).expect("a fingerprint");
    take_back(&main, &wt, &placed, &applied, None).expect("taken back");
    assert!(run(&wt, &["symbolic-ref", "-q", "HEAD"], None).is_err());
    assert_eq!(git(&wt, &["rev-parse", "HEAD"]), prior);
    assert_eq!(git(&wt, &["status", "--porcelain"]), "");
}

#[test]
fn a_worktree_folder_is_named_with_forward_slashes() {
    let (dir, _wt) = source();
    let main = dir.path().join("main");
    git(&main, &["worktree", "add", "-q", "-b", "deep", ".worktrees/a/b"]);
    let made = vec![main.join(".worktrees/a/b").to_string_lossy().into_owned()];
    let moving = moving(&main, &made);
    assert_eq!(moving.len(), 1);
    assert_eq!(moving[0].path, ".worktrees/a/b");
}

#[test]
fn edits_in_the_checkout_count_but_a_worktree_inside_it_does_not() {
    let (dir, _wt) = source();
    let main = dir.path().join("main");
    git(&main, &["worktree", "add", "-q", "-b", "inside", ".worktrees/inside"]);
    fs::write(main.join(".worktrees/inside/a.txt"), "edited there\n").unwrap();
    assert!(!has_edits(&main), "a worktree inside is not the checkout's edit");
    fs::write(main.join("loose.txt"), "new\n").unwrap();
    assert!(has_edits(&main));
    assert!(!has_edits(&dir.path().join("nowhere")), "outside Git");
}
