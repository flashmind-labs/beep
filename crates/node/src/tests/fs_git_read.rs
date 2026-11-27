use super::*;
use crate::agents::work_folder::git;
use std::fs;

pub(crate) use crate::agents::checkout::tests::{has_git, run};

#[test]
fn a_main_folder_comes_from_the_shared_git_dir() {
    let top = Path::new("/home/user/proj/.worktrees/fix");
    assert_eq!(
        main_folder(top, top, Path::new("/home/user/proj/.git")),
        Path::new("/home/user/proj")
    );
    assert_eq!(
        main_folder(top, top, Path::new("/home/user/proj/.git/modules/sub")),
        top
    );
}

#[test]
fn subfolders_and_worktrees_lead_to_the_checkout() {
    if !has_git() {
        eprintln!("skipped: no git");
        return;
    }

    let root = tempfile::tempdir().expect("a temp folder");
    fs::create_dir_all(root.path().join("proj")).expect("a folder");
    let repo = paths::canonicalize(root.path().join("proj")).expect("a real path");
    run(&repo, &["init", "-q"]);
    run(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "https://example.com/org/proj.git",
        ],
    );
    run(&repo, &["commit", "-q", "--allow-empty", "-m", "start"]);
    fs::create_dir_all(repo.join("src/deep")).expect("folders");
    run(&repo, &["worktree", "add", "-q", ".worktrees/fix"]);
    for dir in [
        repo.clone(),
        repo.join("src/deep"),
        repo.join(".worktrees/fix"),
    ] {
        let found = checkout(&dir).expect("a checkout");
        assert_eq!(found.root, repo, "from {}", dir.display());
        assert_eq!(found.origin, "https://example.com/org/proj.git");
    }
    let plain = root.path().join("plain");
    fs::create_dir_all(&plain).expect("a folder");
    // A temp folder may sit inside a checkout on some machines; only a
    // folder git calls outside any is checked.
    if git(&plain, &["rev-parse", "--show-toplevel"]).is_none() {
        assert_eq!(checkout(&plain), None);
    }
}

/// A commit in `repo` made at `seconds`, Unix time.
fn commit_at(repo: &Path, seconds: u64) {
    let date = format!("@{seconds} +0000");
    let out = std::process::Command::new("git")
        .args(["commit", "-q", "--allow-empty", "-m", "change"])
        .current_dir(repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .output()
        .expect("git runs");
    assert!(out.status.success(), "{out:?}");
}

fn new_repo(root: &Path) -> PathBuf {
    fs::create_dir_all(root.join("proj")).expect("a folder");
    let repo = paths::canonicalize(root.join("proj")).expect("a real path");
    run(&repo, &["init", "-q", "-b", "main"]);
    repo
}

#[test]
fn an_answer_is_asked_once_while_its_files_keep_their_stamp() {
    let root = tempfile::tempdir().expect("a temp folder");
    let file = root.path().join("HEAD");
    fs::write(&file, "ref: refs/heads/main\n").expect("a file");
    let stamp = |file: &Path| Stamp {
        git: root.path().to_owned(),
        files: vec![FileStamp::of(file)],
    };
    let kept = Kept::<Option<u64>>::default();
    let asked = std::cell::Cell::new(0);
    let ask = |value| {
        asked.set(asked.get() + 1);
        value
    };
    let key = Path::new("/srv/project");

    // A failure is kept like an answer, so it is not asked again at once.
    assert_eq!(kept.get(key, stamp(&file), || ask(None)), None);
    assert_eq!(kept.get(key, stamp(&file), || ask(Some(1))), None);
    assert_eq!(asked.get(), 1);

    // Git rewrites a ref by renaming a new file over it.
    let next = root.path().join("HEAD.lock");
    fs::write(&next, "ref: refs/heads/next\n").expect("a file");
    fs::rename(&next, &file).expect("renamed");
    assert_eq!(kept.get(key, stamp(&file), || ask(Some(2))), Some(2));
    assert_eq!(kept.get(key, stamp(&file), || ask(Some(3))), Some(2));
    assert_eq!(asked.get(), 2);
}

#[test]
fn a_new_commit_or_branch_switch_shows_in_the_last_commit() {
    if !has_git() {
        eprintln!("skipped: no git");
        return;
    }

    let root = tempfile::tempdir().expect("a temp folder");
    let repo = new_repo(root.path());
    assert_eq!(last_commit(&repo), None, "no commit yet");

    commit_at(&repo, 1_000_000_000);
    assert_eq!(last_commit(&repo), Some(1_000_000_000));
    run(&repo, &["branch", "old"]);
    commit_at(&repo, 1_100_000_000);
    assert_eq!(last_commit(&repo), Some(1_100_000_000));

    run(&repo, &["switch", "-q", "old"]);
    assert_eq!(last_commit(&repo), Some(1_000_000_000));

    // A ref that only lives packed moves too.
    run(&repo, &["pack-refs", "--all"]);
    commit_at(&repo, 1_200_000_000);
    assert_eq!(last_commit(&repo), Some(1_200_000_000));
}

#[test]
fn a_new_remote_or_worktree_shows_in_the_checkout() {
    if !has_git() {
        eprintln!("skipped: no git");
        return;
    }

    let root = tempfile::tempdir().expect("a temp folder");
    let repo = new_repo(root.path());
    commit_at(&repo, 1_000_000_000);
    let before = checkout(&repo).expect("a checkout");
    assert_eq!(before.origin, "");

    run(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "https://example.com/org/proj.git",
        ],
    );
    let after = checkout(&repo).expect("a checkout");
    assert_eq!(after.origin, "https://example.com/org/proj.git");

    run(&repo, &["worktree", "add", "-q", ".worktrees/fix"]);
    let linked = checkout(&repo.join(".worktrees/fix")).expect("a checkout");
    assert_eq!(linked.root, repo);
    assert_eq!(linked.origin, after.origin);
    assert_eq!(
        last_commit(&repo.join(".worktrees/fix")),
        Some(1_000_000_000)
    );
}

#[test]
fn a_folder_with_no_git_above_is_in_no_checkout() {
    let root = tempfile::tempdir().expect("a temp folder");

    // A temp folder may sit inside a checkout on some machines.
    if GitFiles::above(root.path()).is_none() {
        assert_eq!(checkout(root.path()), None);
        assert_eq!(last_commit(root.path()), None);
    }
}

/// HEAD is asked of git once while its files stay as they are, again after a
/// commit, and again each time while there is no commit yet.
#[test]
fn a_head_is_kept_until_a_commit_and_a_missing_one_is_not() {
    if !has_git() {
        eprintln!("skipped: no git");
        return;
    }

    let root = tempfile::tempdir().expect("a temp folder");
    let repo = paths::canonicalize(root.path()).expect("a real path");
    run(&repo, &["init", "-q"]);
    let asks = std::cell::Cell::new(0);
    let read = || {
        head(&repo, || {
            asks.set(asks.get() + 1);
            git(&repo, &["rev-parse", "-q", "--verify", "HEAD^{commit}"])
                .map(|said| said.trim().to_owned())
        })
    };

    assert_eq!(read(), None);
    assert_eq!(read(), None);
    assert_eq!(asks.get(), 2);

    run(&repo, &["commit", "-q", "--allow-empty", "-m", "one"]);
    let first = read().expect("a commit");
    assert_eq!(read().as_deref(), Some(first.as_str()));
    assert_eq!(asks.get(), 3);

    run(&repo, &["commit", "-q", "--allow-empty", "-m", "two"]);
    let second = read().expect("a commit");
    assert_ne!(second, first);
    assert_eq!(asks.get(), 4);
}
