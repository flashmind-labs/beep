//! A proposed background policy, using Git's local transport in place of mesh changes.
//! Only `current_mesh_waits_then_catches_up_after_switching_back` uses live mesh sync.

use super::*;

const PEER_MAIN: &str = "refs/remotes/beep-simulation/main";

#[derive(Debug, PartialEq, Eq)]
struct CheckoutSnapshot {
    head: String,
    branch: String,
    index: String,
    staged: String,
    unstaged: String,
    status: String,
}

fn snapshot(folder: &Path) -> CheckoutSnapshot {
    CheckoutSnapshot {
        head: git(folder, &["rev-parse", "HEAD"]),
        branch: git(folder, &["symbolic-ref", "HEAD"]),
        index: git(folder, &["ls-files", "--stage"]),
        staged: git(folder, &["diff", "--cached", "--binary"]),
        unstaged: git(folder, &["diff", "--binary"]),
        status: git(
            folder,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        ),
    }
}

async fn paused_pair() -> (Node, Node) {
    let (a, b) = synced_repo().await;
    a.pause(&b, true).await;
    b.pause(&a, true).await;
    eventually("both nodes to pause before the prototype runs", || {
        a.status().state == State::Paused && b.status().state == State::Paused
    })
    .await;
    (a, b)
}

/// Receiving a peer's history changes its reference, never a local branch or index.
fn receive_main(source: &Node, target: &Node) -> String {
    git(
        &target.folder(),
        &[
            "fetch",
            "--no-tags",
            "--no-write-fetch-head",
            &source.folder().to_string_lossy(),
            &format!("refs/heads/main:{PEER_MAIN}"),
        ],
    );
    git(&target.folder(), &["rev-parse", PEER_MAIN])
}

/// Git rejects a non-fast-forward or a branch checked out in any local worktree.
fn advance_inactive_main(node: &Node) -> std::process::Output {
    try_git(
        &node.folder(),
        &[
            "fetch",
            "--no-tags",
            "--no-write-fetch-head",
            ".",
            &format!("{PEER_MAIN}:refs/heads/main"),
        ],
    )
}

fn assert_advanced(node: &Node, target: &str) {
    let result = advance_inactive_main(node);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(git(&node.folder(), &["rev-parse", "main"]), target);
}

#[tokio::test(flavor = "multi_thread")]
async fn current_mesh_waits_then_catches_up_after_switching_back() {
    let (a, b) = paused_pair().await;
    let base = head(&b).unwrap();
    git(&b.folder(), &["switch", "-c", "feature"]);
    let newest = commit(&a, "README.md", "committed on main\n");
    write(&a.path("local-only.txt"), "uncommitted work\n");
    a.pause(&b, false).await;
    b.pause(&a, false).await;
    eventually("different branches to wait", || {
        b.status()
            .error
            .as_deref()
            .is_some_and(|error| error.contains("between branches"))
    })
    .await;
    assert_eq!(git(&b.folder(), &["rev-parse", "main"]), base);
    assert_eq!(git(&b.folder(), &["branch", "--show-current"]), "feature");
    git(&b.folder(), &["switch", "main"]);
    eventually("main to catch up after switching back", || {
        head(&b).as_deref() == Some(newest.as_str()) && is_clean(&b)
    })
    .await;
    assert_eq!(git(&b.folder(), &["rev-parse", "feature"]), base);
    assert!(!b.path("local-only.txt").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn prototype_syncs_inactive_main_without_touching_dirty_feature_work() {
    let (a, b) = paused_pair().await;
    git(&b.folder(), &["switch", "-c", "feature"]);
    commit(&b, "feature.txt", "feature commit\n");
    write(&b.path("README.md"), "staged feature edit\n");
    git(&b.folder(), &["add", "README.md"]);
    write(&b.path("README.md"), "unstaged feature edit\n");
    write(&b.path("local.bin"), b"local\0binary");
    let before = snapshot(&b.folder());
    let first = commit(&a, "README.md", "first main commit\n");
    assert_eq!(receive_main(&a, &b), first);
    assert_advanced(&b, &first);
    assert_eq!(snapshot(&b.folder()), before);
    write(&b.path("README.md"), "continued feature edit\n");
    let continued = snapshot(&b.folder());
    let newest = commit(&a, "next.txt", "second main commit\n");
    write(&a.path("not-committed.txt"), "stays on source\n");
    assert_eq!(receive_main(&a, &b), newest);
    assert_advanced(&b, &newest);
    assert_eq!(snapshot(&b.folder()), continued);
    assert_eq!(
        read(&b.path("local.bin")).as_deref(),
        Some(b"local\0binary".as_slice())
    );
    assert!(!b.path("next.txt").exists());
    assert!(!b.path("not-committed.txt").exists());
    assert_eq!(
        git(&b.folder(), &["show", "main:next.txt"]),
        "second main commit"
    );
    assert_sound(&b);
}

#[tokio::test(flavor = "multi_thread")]
async fn prototype_cached_commits_advance_main_even_after_the_peer_disconnects() {
    let (mut a, b) = paused_pair().await;
    git(&b.folder(), &["switch", "-c", "feature"]);
    let feature = commit(&b, "README.md", "feature version\n");
    let before = snapshot(&b.folder());
    let newest = commit(&a, "README.md", "main version\n");
    assert_eq!(receive_main(&a, &b), newest);
    a.stop().await;
    assert_advanced(&b, &newest);
    assert_eq!(snapshot(&b.folder()), before);
    git(&b.folder(), &["switch", "main"]);
    assert_eq!(head(&b).as_deref(), Some(newest.as_str()));
    assert_eq!(
        read(&b.path("README.md")).as_deref(),
        Some(b"main version\n".as_slice())
    );
    assert_eq!(git(&b.folder(), &["rev-parse", "feature"]), feature);
    assert!(is_clean(&b));
}

#[tokio::test(flavor = "multi_thread")]
async fn prototype_divergent_main_keeps_local_history_but_receives_peer_commits() {
    let (a, b) = paused_pair().await;
    let ours = commit(&b, "local.txt", "local main commit\n");
    git(&b.folder(), &["switch", "-c", "feature"]);
    let before = snapshot(&b.folder());
    let theirs = commit(&a, "remote.txt", "peer main commit\n");
    assert_eq!(receive_main(&a, &b), theirs);
    let result = advance_inactive_main(&b);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("non-fast-forward"));
    assert_eq!(git(&b.folder(), &["rev-parse", "main"]), ours);
    assert_eq!(git(&b.folder(), &["rev-parse", PEER_MAIN]), theirs);
    assert_eq!(snapshot(&b.folder()), before);
    assert_sound(&b);
}

#[tokio::test(flavor = "multi_thread")]
async fn prototype_main_in_another_worktree_is_not_advanced() {
    let (a, b) = paused_pair().await;
    let base = head(&b).unwrap();
    git(&b.folder(), &["switch", "-c", "feature"]);
    write(&b.path(".git/info/exclude"), ".worktrees/\n");
    git(&b.folder(), &["worktree", "add", ".worktrees/main", "main"]);
    let worktree = b.path(".worktrees/main");
    write(&worktree.join("README.md"), "dirty linked worktree\n");
    let checkout = snapshot(&b.folder());
    let linked = snapshot(&worktree);
    let newest = commit(&a, "README.md", "peer main commit\n");
    assert_eq!(receive_main(&a, &b), newest);
    let result = advance_inactive_main(&b);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("checked out"));
    assert_eq!(git(&b.folder(), &["rev-parse", "main"]), base);
    assert_eq!(snapshot(&b.folder()), checkout);
    assert_eq!(snapshot(&worktree), linked);
    assert_eq!(git(&b.folder(), &["rev-parse", PEER_MAIN]), newest);
}

#[tokio::test(flavor = "multi_thread")]
async fn prototype_active_main_only_receives_objects_and_preserves_local_edits() {
    let (a, b) = paused_pair().await;
    write(&b.path("README.md"), "staged on main\n");
    git(&b.folder(), &["add", "README.md"]);
    write(&b.path("README.md"), "unstaged on main\n");
    let before = snapshot(&b.folder());
    let newest = commit(&a, "README.md", "peer main commit\n");
    assert_eq!(receive_main(&a, &b), newest);
    let result = advance_inactive_main(&b);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("checked out"));
    assert_eq!(snapshot(&b.folder()), before);
    assert_eq!(git(&b.folder(), &["rev-parse", PEER_MAIN]), newest);
}

#[tokio::test(flavor = "multi_thread")]
async fn prototype_syncs_main_while_both_project_checkouts_are_on_other_branches() {
    let (a, b) = paused_pair().await;
    git(&a.folder(), &["switch", "-c", "source-feature"]);
    git(&b.folder(), &["switch", "-c", "target-feature"]);
    write(&a.path(".git/info/exclude"), ".worktrees/\n");
    git(&a.folder(), &["worktree", "add", ".worktrees/main", "main"]);
    let worktree = a.path(".worktrees/main");
    write(
        &worktree.join("README.md"),
        "committed from a linked worktree\n",
    );
    git(&worktree, &["add", "README.md"]);
    git(&worktree, &["commit", "-q", "-m", "main update"]);
    let newest = git(&worktree, &["rev-parse", "HEAD"]);
    let source = snapshot(&a.folder());
    let target = snapshot(&b.folder());
    assert_eq!(receive_main(&a, &b), newest);
    assert_advanced(&b, &newest);
    assert_eq!(snapshot(&a.folder()), source);
    assert_eq!(snapshot(&b.folder()), target);
    git(&b.folder(), &["switch", "main"]);
    assert_eq!(head(&b).as_deref(), Some(newest.as_str()));
    assert_eq!(
        read(&b.path("README.md")).as_deref(),
        Some(b"committed from a linked worktree\n".as_slice())
    );
}
