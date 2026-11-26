use super::*;
use crate::files::git_read::tests::{has_git, run};
use crate::fs::paths;
use std::{fs, path::PathBuf};

#[test]
fn main_checkouts_are_found_and_the_rest_left_out() {
    if !has_git() {
        eprintln!("skipped: no git");
        return;
    }

    let temp = tempfile::tempdir().expect("a temp folder");
    let home = paths::canonicalize(temp.path()).expect("a real path");
    let repo = |rel: &str| {
        let dir = home.join(rel);
        fs::create_dir_all(&dir).expect("a folder");
        run(&dir, &["init", "-q"]);
        dir
    };
    let nasty = repo("code/games/nasty");
    run(
        &nasty,
        &[
            "remote",
            "add",
            "origin",
            "https://example.com/me/nasty.git",
        ],
    );
    run(&nasty, &["commit", "-q", "--allow-empty", "-m", "start"]);
    run(&nasty, &["worktree", "add", "-q", "../nasty-fix"]);
    let notes = repo("notes");
    repo("notes/vendored");
    repo("code/web/node_modules/dep");
    repo(".config/plugin");
    repo("Pictures/shared");
    repo("Music/stems");
    repo("a/b/c/d/too-deep");

    let mut found = checkouts(&home, MAX_DEPTH, WALK_FOR).found;
    found.sort_by(|a, b| a.root.cmp(&b.root));
    let roots: Vec<&Path> = found.iter().map(|one| one.root.as_path()).collect();
    assert_eq!(roots, [nasty.as_path(), notes.as_path()]);
    assert_eq!(found[0].origin, "https://example.com/me/nasty.git");
    assert_eq!(found[1].origin, "");
}

#[test]
fn a_whole_walk_is_saved_for_the_next_start() {
    let temp = tempfile::tempdir().expect("a temp folder");
    let (home, dir) = (temp.path().join("home"), temp.path().join("node"));
    let repo = home.join("code/app");
    fs::create_dir_all(repo.join(".git")).expect("a checkout");
    fs::create_dir_all(&dir).expect("the node's folder");

    assert_eq!(saved_checkouts(&dir), None);
    let first = walk(&home, &dir);
    assert!(first.whole);
    assert_eq!(first.found.len(), 1);
    assert_eq!(first.found[0].root, repo);
    fs::remove_dir_all(&home).expect("home gone");
    assert_eq!(saved_checkouts(&dir), Some(first.found));
}

#[test]
fn a_rescan_adds_new_checkouts_and_drops_gone_ones() {
    let temp = tempfile::tempdir().expect("a temp folder");
    let (home, dir) = (temp.path().join("home"), temp.path().join("node"));
    let (kept, gone, new) = (home.join("code/app"), home.join("old"), home.join("ops"));

    for repo in [&kept, &gone] {
        fs::create_dir_all(repo.join(".git")).expect("a checkout");
    }

    fs::create_dir_all(&dir).expect("the node's folder");
    let known = walk(&home, &dir).found;
    assert_eq!(known.len(), 2);

    fs::remove_dir_all(&gone).expect("a checkout removed");
    fs::create_dir_all(new.join(".git")).expect("a checkout made");
    let mut roots: Vec<PathBuf> = rescan(&home, &dir, &known)
        .into_iter()
        .map(|one| one.root)
        .collect();
    roots.sort();
    assert_eq!(roots, [kept, new]);
    assert_eq!(saved_checkouts(&dir).map(|saved| saved.len()), Some(2));
}
