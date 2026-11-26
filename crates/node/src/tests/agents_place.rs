use super::*;
use crate::agents::checkout::tests::{has_git, run};
use crate::fs::paths;
use serde_json::json;
use std::fs;

#[test]
fn a_linked_worktree_is_told_from_the_main_checkout() {
    if !has_git() {
        eprintln!("skipped: no git");
        return;
    }

    let root = tempfile::tempdir().expect("a temp folder");
    fs::create_dir_all(root.path().join("proj")).expect("a folder");
    let repo = paths::canonicalize(root.path().join("proj")).expect("a real path");
    run(&repo, &["init", "-q", "-b", "main"]);
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
    run(
        &repo,
        &["worktree", "add", "-q", "-b", "fix", ".worktrees/fix"],
    );

    let main = of_folder(&repo.join("src/deep")).expect("a place");
    assert!(!main.linked);
    assert_eq!(main.branch.as_deref(), Some("main"));
    assert_eq!(main.main, repo.to_string_lossy());

    let tree = repo.join(".worktrees/fix");
    // A file that is not there yet counts for its folder.
    let linked = of_folder(&tree.join("new/file.rs")).expect("a place");
    assert!(linked.linked);
    assert_eq!(linked.branch.as_deref(), Some("fix"));
    assert_eq!(linked.main, repo.to_string_lossy());
    assert_eq!(linked.origin, "https://example.com/org/proj.git");
    assert_eq!(linked.folder, tree.to_string_lossy());

    // A branch switch shows at once, though the answer is kept.
    run(&tree, &["switch", "-q", "-c", "next"]);
    let switched = of_folder(&tree).expect("a place");
    assert_eq!(switched.branch.as_deref(), Some("next"));
}

#[test]
fn a_call_names_its_folder_by_workdir_or_the_file_it_edits() {
    let base = Path::new("/srv/project");
    let workdir = json!({"cmd": "ls", "workdir": "/srv/project/.worktrees/fix"});
    assert_eq!(
        call_folder(base, Some(&workdir), None),
        Some(PathBuf::from("/srv/project/.worktrees/fix"))
    );
    let relative = json!({"workdir": "sub"});
    assert_eq!(
        call_folder(base, Some(&relative), None),
        Some(PathBuf::from("/srv/project/sub"))
    );
    let file = Path::new("/srv/project/.worktrees/fix/src/lib.rs");
    assert_eq!(
        call_folder(base, None, Some(file)),
        Some(PathBuf::from("/srv/project/.worktrees/fix/src"))
    );
    assert_eq!(call_folder(base, Some(&json!({"cmd": "ls"})), None), None);
    let remote = json!({"machine": "build-box", "cwd": "/srv/other"});
    assert_eq!(call_folder(base, Some(&remote), None), None);
}
