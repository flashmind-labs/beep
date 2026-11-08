use super::*;
use beep_proto::node::sync::ignored_project;
use std::process::Command;

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_folder_must_be_wholly_ignored_and_staged_or_committed_files_block_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.name", "Test"]);
    git(root, &["config", "user.email", "test@example.com"]);
    fs::write(root.join(".gitignore"), "models/\n").unwrap();
    git(root, &["add", ".gitignore"]);
    git(root, &["commit", "-qm", "baseline"]);
    fs::create_dir(root.join("models")).unwrap();
    fs::write(root.join("models/scene.blend"), "model").unwrap();
    let scope = IgnoredRoot::parse(
        &ignored_project("acme/widget", "models"),
        &root.join("models"),
    )
    .unwrap()
    .unwrap();
    assert_eq!(scope.repository, root);
    scope.check().unwrap();
    fs::write(root.join(".gitignore"), "models/*\n!models/keep.txt\n").unwrap();
    assert!(
        scope.check().is_err(),
        "partially ignored folders are not selected wholesale"
    );
    fs::write(root.join(".gitignore"), "models/\n").unwrap();
    git(root, &["add", "-f", "models/scene.blend"]);
    assert!(scope.guard().is_err(), "staged content belongs to Git");
    git(root, &["commit", "-qm", "track model"]);
    assert!(scope.check().is_err());
    git(root, &["rm", "--cached", "models/scene.blend"]);
    assert!(
        scope.check().is_err(),
        "HEAD still tracks a staged deletion"
    );
    assert!(!root.join(".git/index.lock").exists());
}
