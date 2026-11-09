use super::*;
use std::process::Command;
use tempfile::TempDir;

fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn checkouts() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    let target = temp.path().join("target");
    fs::create_dir(&source).unwrap();
    git(&source, &["init", "-q", "-b", "main"]);
    fs::write(source.join("tracked.txt"), "base\n").unwrap();
    fs::write(source.join("removed.txt"), "remove me\n").unwrap();
    fs::write(source.join(".gitignore"), "ignored.txt\n").unwrap();
    git(&source, &["add", "."]);
    git(&source, &["commit", "-qm", "base"]);
    git(temp.path(), &["clone", "-q", "source", "target"]);
    (temp, source, target)
}

#[test]
fn handoff_carries_edits_deletes_binary_and_untracked_files_without_changing_source() {
    let (_temp, source, target) = checkouts();
    fs::write(source.join("tracked.txt"), "staged\n").unwrap();
    git(&source, &["add", "tracked.txt"]);
    fs::write(source.join("tracked.txt"), "staged and unstaged\n").unwrap();
    fs::write(source.join("new file.bin"), [0, 1, 255, 0]).unwrap();
    fs::write(source.join("ignored.txt"), "private local content").unwrap();
    fs::remove_file(source.join("removed.txt")).unwrap();
    let before = git(&source, &["diff", "--cached", "--binary"]);
    let status = git(&source, &["status", "--porcelain"]);
    apply(&target, snapshot(&source).unwrap()).unwrap();
    assert_eq!(
        fs::read(target.join("tracked.txt")).unwrap(),
        b"staged and unstaged\n"
    );
    assert_eq!(
        fs::read(target.join("new file.bin")).unwrap(),
        [0, 1, 255, 0]
    );
    assert!(!target.join("removed.txt").exists());
    assert!(!target.join("ignored.txt").exists());
    assert_eq!(git(&source, &["diff", "--cached", "--binary"]), before);
    assert_eq!(git(&source, &["status", "--porcelain"]), status);
    assert!(git(&target, &["diff", "--cached"]).is_empty());
    assert!(!target.join(".git/index.lock").exists());
}

#[test]
fn dirty_destinations_and_different_commits_are_refused_without_changing_files() {
    for dirty in ["unstaged", "staged", "untracked", "commit"] {
        let (_temp, source, target) = checkouts();
        fs::write(source.join("tracked.txt"), "incoming\n").unwrap();
        let incoming = snapshot(&source).unwrap();
        let file = if dirty == "untracked" {
            "local.txt"
        } else {
            "tracked.txt"
        };
        fs::write(target.join(file), "destination work\n").unwrap();

        if dirty == "staged" || dirty == "commit" {
            git(&target, &["add", file]);
        }

        if dirty == "commit" {
            git(&target, &["commit", "-qm", "local"]);
        }

        let before = git(&target, &["status", "--porcelain"]);
        let index = git(&target, &["diff", "--cached", "--binary"]);
        assert!(apply(&target, incoming).is_err(), "{dirty}");
        assert_eq!(fs::read(target.join(file)).unwrap(), b"destination work\n");
        assert_eq!(git(&target, &["status", "--porcelain"]), before);
        assert_eq!(git(&target, &["diff", "--cached", "--binary"]), index);
        assert!(!target.join(".git/index.lock").exists());
    }
}

#[test]
fn ignored_files_force_added_to_the_index_are_included() {
    let (_temp, source, target) = checkouts();
    fs::write(source.join("ignored.txt"), "intentionally staged\n").unwrap();
    git(&source, &["add", "-f", "ignored.txt"]);
    let index = fs::read(source.join(".git/index")).unwrap();
    apply(&target, snapshot(&source).unwrap()).unwrap();
    assert_eq!(
        fs::read(target.join("ignored.txt")).unwrap(),
        b"intentionally staged\n"
    );
    assert_eq!(fs::read(source.join(".git/index")).unwrap(), index);
}

#[test]
fn a_collision_with_an_ignored_file_applies_nothing() {
    let (_temp, source, target) = checkouts();
    fs::write(source.join("tracked.txt"), "incoming\n").unwrap();
    fs::write(source.join("ignored.txt"), "incoming ignored\n").unwrap();
    git(&source, &["add", "-f", "ignored.txt"]);
    fs::write(target.join("ignored.txt"), "local ignored\n").unwrap();
    assert!(apply(&target, snapshot(&source).unwrap()).is_err());
    assert_eq!(fs::read(target.join("tracked.txt")).unwrap(), b"base\n");
    assert_eq!(
        fs::read(target.join("ignored.txt")).unwrap(),
        b"local ignored\n"
    );
    assert!(!target.join(".git/index.lock").exists());
}

#[test]
fn submodule_work_is_refused_instead_of_silently_left_out() {
    let (temp, source, _target) = checkouts();
    let module = temp.path().join("module");
    fs::create_dir(&module).unwrap();
    git(&module, &["init", "-q", "-b", "main"]);
    fs::write(module.join("file.txt"), "base\n").unwrap();
    git(&module, &["add", "."]);
    git(&module, &["commit", "-qm", "base"]);
    git(
        &source,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "../module",
            "module",
        ],
    );
    git(&source, &["commit", "-qm", "add module"]);
    fs::write(source.join("module/file.txt"), "local submodule edit\n").unwrap();
    let error = snapshot(&source).unwrap_err();
    assert!(error.to_string().contains("Submodule changes"));
    assert_eq!(
        fs::read(source.join("module/file.txt")).unwrap(),
        b"local submodule edit\n"
    );
}

#[test]
fn linked_worktrees_and_nested_repositories_stay_on_the_source() {
    let (_temp, source, target) = checkouts();
    git(
        &source,
        &["worktree", "add", "-qb", "feature", ".worktrees/feature"],
    );
    git(&source, &["worktree", "add", "-qb", "another", "nested"]);
    fs::write(source.join("tracked.txt"), "incoming\n").unwrap();
    fs::write(source.join("nested/local.txt"), "local nested work\n").unwrap();
    apply(&target, snapshot(&source).unwrap()).unwrap();
    assert_eq!(fs::read(target.join("tracked.txt")).unwrap(), b"incoming\n");
    assert!(!target.join(".worktrees").exists());
    assert!(!target.join("nested").exists());
    assert_eq!(
        fs::read(source.join("nested/local.txt")).unwrap(),
        b"local nested work\n"
    );
}
