use super::*;

#[test]
fn advertisements_exclude_private_names_and_duplicate_branches() {
    let commit = Oid::from([1; 32]);
    for name in [
        "refs/stash",
        "refs/beep-recovery/saved",
        "refs/remotes/peer/main",
    ] {
        assert!(
            BranchTip::validate(&[BranchTip {
                name: name.into(),
                commit
            }])
            .is_err()
        );
    }
    let main = BranchTip {
        name: "refs/heads/main".into(),
        commit,
    };
    assert!(BranchTip::validate(&[main.clone(), main]).is_err());
}

#[test]
fn inactive_updates_block_checkout_in_existing_worktrees_and_release_only_their_locks() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repo {
        root: temp.path().to_owned(),
    };
    repo.run(&["init", "-q", "-b", "main"]).unwrap();
    std::fs::write(temp.path().join("README.md"), "baseline\n").unwrap();
    repo.run(&["add", "README.md"]).unwrap();
    repo.run(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.invalid",
        "commit",
        "-q",
        "-m",
        "baseline",
    ])
    .unwrap();
    repo.run(&["branch", "feature"]).unwrap();
    repo.run(&["branch", "linked"]).unwrap();
    repo.run(&["worktree", "add", ".worktrees/linked", "linked"])
        .unwrap();
    let linked_admin = temp.path().join(".git/worktrees/linked");
    let linked_root = temp.path().join(".worktrees/linked");
    let guard = WriteGuard::for_branches(temp.path()).unwrap();
    assert!(repo.run(&["switch", "feature"]).is_err());
    let output = repo
        .git()
        .arg("--git-dir")
        .arg(&linked_admin)
        .arg("--work-tree")
        .arg(&linked_root)
        .args(["switch", "feature"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("index.lock"));
    assert_eq!(repo.head_ref().as_deref(), Some("refs/heads/main"));
    assert_eq!(
        std::fs::read_to_string(linked_admin.join("HEAD"))
            .unwrap()
            .trim(),
        "ref: refs/heads/linked"
    );
    drop(guard);
    assert!(!temp.path().join(".git/index.lock").exists());
    assert!(!linked_admin.join("index.lock").exists());
    std::fs::write(linked_admin.join("index.lock"), "another operation").unwrap();
    assert!(WriteGuard::for_branches(temp.path()).is_err());
    assert!(!temp.path().join(".git/index.lock").exists());
    assert_eq!(
        std::fs::read_to_string(linked_admin.join("index.lock")).unwrap(),
        "another operation"
    );
}

#[test]
fn a_repeated_catalog_reports_only_the_tips_that_moved() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repo {
        root: temp.path().to_owned(),
    };
    let commit = |message: &str| {
        repo.run(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            message,
        ])
        .unwrap();
        repo.head().unwrap()
    };
    repo.run(&["init", "-q", "-b", "main"]).unwrap();
    let first = commit("first");
    let second = commit("second");
    let peer = iroh::SecretKey::from_bytes(&[7; 32]).public();
    let tip = |name: &str, commit| BranchTip {
        name: format!("refs/heads/{name}"),
        commit,
    };

    let catalog = [tip("main", first), tip("topic", second)];
    assert_eq!(repo.prepare_peer_refs(&peer, &catalog).unwrap(), catalog);
    for tip in &catalog {
        repo.keep_peer_branch(&peer, tip).unwrap();
    }

    assert!(repo.prepare_peer_refs(&peer, &catalog).unwrap().is_empty());

    let moved = [tip("main", second)];
    assert_eq!(repo.prepare_peer_refs(&peer, &moved).unwrap(), moved);
    assert_eq!(repo.cached_branches(&peer).unwrap(), vec![tip("main", first)]);
}
