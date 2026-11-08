//! Branch-aware commit exchange over the real node-to-node mesh.

use super::*;

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
    eventually("both nodes to pause before the branch tests run", || {
        a.status().state == State::Paused && b.status().state == State::Paused
    })
    .await;
    (a, b)
}

fn ref_commit(node: &Node, reference: &str) -> Option<String> {
    let out = try_git(&node.folder(), &["rev-parse", "-q", "--verify", reference]);
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn peer_ref(source: &Node, branch: &str) -> String {
    format!("refs/remotes/beep/{}/{branch}", source.addr().id)
}

async fn resume_pair(a: &Node, b: &Node) {
    a.pause(b, false).await;
    b.pause(a, false).await;
}

async fn main_reaches(node: &Node, target: &str) {
    eventually("inactive main to receive the peer's commit", || {
        ref_commit(node, "refs/heads/main").as_deref() == Some(target)
    })
    .await;
}

fn exclude_worktrees(node: &Node) {
    let path = node.path(".git/info/exclude");
    let mut excludes = fs::read_to_string(&path).unwrap_or_default();
    excludes.push_str("\n.worktrees/\n");
    write(&path, excludes);
}

#[tokio::test(flavor = "multi_thread")]
async fn mesh_syncs_inactive_main_then_switches_back_to_the_latest_commit() {
    let (a, b) = paused_pair().await;
    let base = head(&b).unwrap();
    git(&b.folder(), &["switch", "-c", "feature"]);
    let newest = commit(&a, "README.md", "committed on main\n");
    write(&a.path("local-only.txt"), "uncommitted work\n");
    resume_pair(&a, &b).await;
    main_reaches(&b, &newest).await;
    assert_eq!(head(&b).as_deref(), Some(base.as_str()));
    assert_eq!(
        read(&b.path("README.md")).as_deref(),
        Some(b"first\n".as_slice())
    );
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
async fn mesh_syncs_inactive_main_without_touching_dirty_feature_work() {
    let (a, b) = paused_pair().await;
    git(&b.folder(), &["switch", "-c", "feature"]);
    commit(&b, "feature.txt", "feature commit\n");
    write(&b.path("README.md"), "staged feature edit\n");
    git(&b.folder(), &["add", "README.md"]);
    write(&b.path("README.md"), "unstaged feature edit\n");
    write(&b.path("local.bin"), b"local\0binary");
    let before = snapshot(&b.folder());
    let first = commit(&a, "README.md", "first main commit\n");
    resume_pair(&a, &b).await;
    main_reaches(&b, &first).await;
    assert_eq!(snapshot(&b.folder()), before);
    write(&b.path("README.md"), "continued feature edit\n");
    let continued = snapshot(&b.folder());
    let newest = commit(&a, "next.txt", "second main commit\n");
    write(&a.path("not-committed.txt"), "stays on source\n");
    resume_pair(&a, &b).await;
    main_reaches(&b, &newest).await;
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
async fn mesh_cached_commits_advance_main_even_after_the_peer_disconnects() {
    let (mut a, b) = paused_pair().await;
    git(&b.folder(), &["switch", "-c", "feature"]);
    let feature = commit(&b, "README.md", "feature version\n");
    let before = snapshot(&b.folder());
    let newest = commit(&a, "README.md", "main version\n");
    resume_pair(&a, &b).await;
    main_reaches(&b, &newest).await;
    a.stop().await;
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
async fn mesh_divergent_main_keeps_local_history_but_receives_peer_commits() {
    let (a, b) = paused_pair().await;
    let ours = commit(&b, "local.txt", "local main commit\n");
    git(&b.folder(), &["switch", "-c", "feature"]);
    let before = snapshot(&b.folder());
    let theirs = commit(&a, "remote.txt", "peer main commit\n");
    resume_pair(&a, &b).await;
    eventually(
        "divergent main to retain peer commits and request a merge",
        || {
            ref_commit(&b, &peer_ref(&a, "main")).as_deref() == Some(theirs.as_str())
                && b.status().error.as_deref().is_some_and(|error| {
                    error.contains("diverged") && error.contains("refs/heads/main")
                })
        },
    )
    .await;
    assert_eq!(git(&b.folder(), &["rev-parse", "main"]), ours);
    assert_eq!(
        git(&b.folder(), &["rev-parse", &peer_ref(&a, "main")]),
        theirs
    );
    assert_eq!(snapshot(&b.folder()), before);
    assert_sound(&b);
}

#[tokio::test(flavor = "multi_thread")]
async fn mesh_main_in_another_worktree_is_not_advanced() {
    let (mut a, mut b) = paused_pair().await;
    let base = head(&b).unwrap();
    git(&b.folder(), &["switch", "-c", "feature"]);
    exclude_worktrees(&b);
    git(&b.folder(), &["worktree", "add", ".worktrees/main", "main"]);
    let worktree = b.path(".worktrees/main");
    write(&worktree.join("README.md"), "dirty linked worktree\n");
    let checkout = snapshot(&b.folder());
    let linked = snapshot(&worktree);
    let newest = commit(&a, "README.md", "peer main commit\n");
    resume_pair(&a, &b).await;
    eventually(
        "the linked worktree to block advancement while receiving commits",
        || {
            ref_commit(&b, &peer_ref(&a, "main")).as_deref() == Some(newest.as_str())
                && b.status()
                    .error
                    .as_deref()
                    .is_some_and(|error| error.contains("checked out"))
        },
    )
    .await;
    assert_eq!(git(&b.folder(), &["rev-parse", "main"]), base);
    assert_eq!(snapshot(&b.folder()), checkout);
    assert_eq!(snapshot(&worktree), linked);
    assert_eq!(
        git(&b.folder(), &["rev-parse", &peer_ref(&a, "main")]),
        newest
    );
    let peer = a.addr();
    a.stop().await;
    b.stop().await;
    b.start().await;
    assert!(b.configure(vec![peer]).await.is_empty());
    write(&worktree.join("README.md"), "first\n");
    git(&b.folder(), &["worktree", "remove", ".worktrees/main"]);
    b.running().transfers.syncs().rescan(PROJECT).await.unwrap();
    main_reaches(&b, &newest).await;
    assert_eq!(snapshot(&b.folder()), checkout);
}

#[tokio::test(flavor = "multi_thread")]
async fn mesh_active_main_only_receives_objects_and_preserves_local_edits() {
    let (a, b) = paused_pair().await;
    write(&b.path("README.md"), "staged on main\n");
    git(&b.folder(), &["add", "README.md"]);
    write(&b.path("README.md"), "unstaged on main\n");
    let before = snapshot(&b.folder());
    let newest = commit(&a, "README.md", "peer main commit\n");
    resume_pair(&a, &b).await;
    eventually(
        "dirty main to receive commits while preserving its edits",
        || {
            ref_commit(&b, &peer_ref(&a, "main")).as_deref() == Some(newest.as_str())
                && b.status()
                    .error
                    .as_deref()
                    .is_some_and(|error| error.contains("staged, unstaged and untracked"))
        },
    )
    .await;
    assert_eq!(snapshot(&b.folder()), before);
    assert_eq!(
        git(&b.folder(), &["rev-parse", &peer_ref(&a, "main")]),
        newest
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn mesh_syncs_main_while_both_project_checkouts_are_on_other_branches() {
    let (a, b) = paused_pair().await;
    git(&a.folder(), &["switch", "-c", "source-feature"]);
    git(&b.folder(), &["switch", "-c", "target-feature"]);
    exclude_worktrees(&a);
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
    resume_pair(&a, &b).await;
    main_reaches(&b, &newest).await;
    assert_eq!(snapshot(&a.folder()), source);
    assert_eq!(snapshot(&b.folder()), target);
    git(&b.folder(), &["switch", "main"]);
    assert_eq!(head(&b).as_deref(), Some(newest.as_str()));
    assert_eq!(
        read(&b.path("README.md")).as_deref(),
        Some(b"committed from a linked worktree\n".as_slice())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn mesh_receives_unknown_branches_without_creating_local_branches_or_switching() {
    let (a, b) = paused_pair().await;
    git(&a.folder(), &["switch", "-c", "new-feature"]);
    let tip = commit(&a, "new.txt", "new feature commit\n");
    let before = snapshot(&b.folder());
    resume_pair(&a, &b).await;
    eventually("new feature history in the peer namespace", || {
        ref_commit(&b, &peer_ref(&a, "new-feature")).as_deref() == Some(tip.as_str())
    })
    .await;
    assert_eq!(snapshot(&b.folder()), before);
    assert_eq!(ref_commit(&b, "refs/heads/new-feature"), None);
    assert!(!b.path("new.txt").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn mesh_peer_branch_prefix_changes_keep_received_history_and_do_not_block_sync() {
    let (a, b) = paused_pair().await;
    git(&a.folder(), &["branch", "topic"]);
    resume_pair(&a, &b).await;
    eventually("the first peer branch", || {
        ref_commit(&b, &peer_ref(&a, "topic")).is_some()
    })
    .await;
    git(&a.folder(), &["branch", "-d", "topic"]);
    git(&a.folder(), &["switch", "-c", "topic/next"]);
    let next = commit(&a, "next.txt", "next branch\n");
    eventually("the replacement peer branch", || {
        ref_commit(&b, &peer_ref(&a, "topic/next")).as_deref() == Some(next.as_str())
            && ref_commit(&b, &peer_ref(&a, "topic")).is_none()
    })
    .await;
    assert!(
        !git(
            &b.folder(),
            &[
                "for-each-ref",
                "--format=%(refname)",
                "refs/beep-sync/retained/"
            ]
        )
        .is_empty()
    );
    assert_eq!(git(&b.folder(), &["branch", "--show-current"]), "main");
    assert!(!b.path("next.txt").exists());
    assert_sound(&b);
}
