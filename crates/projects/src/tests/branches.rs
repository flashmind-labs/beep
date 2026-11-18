use super::*;
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

fn branch(name: &str, ahead: u32, worktree: Option<&str>) -> ProjectBranch {
    ProjectBranch {
        name: name.into(),
        worktree: worktree.map(str::to_owned),
        ahead,
        behind: 0,
        head: "abc".into(),
        subject: String::new(),
        committed_at: SystemTime::UNIX_EPOCH,
        local: true,
        pushed: true,
    }
}

fn pr(number: u64, head: &str) -> OpenPr {
    OpenPr {
        number,
        title: format!("PR {number}"),
        head: head.into(),
    }
}

#[test]
fn branches_with_work_keeps_pr_heads_and_ahead() {
    let branches = [
        branch("main", 0, Some("/srv/project")),
        branch("login-copy", 0, None),
        branch("spike", 3, None),
        branch("stale", 0, None),
        branch("ios", 0, Some("/srv/project/.worktrees/ios")),
    ];
    let prs = [pr(41, "login-copy"), pr(52, "retry-flag")];
    let got = branches_with_work(&branches, &prs, "main");
    let names: Vec<&str> = got.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["login-copy", "spike", "ios", "retry-flag", "main"]);
    assert_eq!(got[0].pr, Some(41));
    assert_eq!(got[0].pr_title, "PR 41");
    assert!(got[0].in_checkout);
    assert_eq!(got[3].pr, Some(52));
    assert!(!got[3].in_checkout);
    assert_eq!(got[4].worktree.as_deref(), Some("/srv/project"));
    assert!(branches_with_work(&[], &[], "main").is_empty());
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Runs the script as a node's shell would and gives its exit code.
fn sh(script: &str) -> Option<i32> {
    Command::new("sh")
        .arg("-c")
        .arg(script)
        .output()
        .expect("sh runs")
        .status
        .code()
}

#[test]
fn the_worktree_script_fetches_from_the_remote_that_has_the_branch() {
    let tmp = TempDir::new().expect("a temp dir");
    let root = tmp.path();
    git(root, &["init", "--bare", "-b", "main", "origin.git"]);
    git(root, &["init", "--bare", "-b", "main", "fork.git"]);
    let work = root.join("work");
    git(root, &["clone", "origin.git", "work"]);
    git(&work, &["checkout", "-b", "main"]);
    fs::write(work.join("README.md"), "widget\n").expect("a file");
    git(&work, &["add", "README.md"]);
    git(&work, &["commit", "-m", "First"]);
    git(&work, &["push", "origin", "main"]);
    git(&work, &["checkout", "-b", "beep/fix"]);
    git(&work, &["commit", "--allow-empty", "-m", "Fix"]);
    git(&work, &["push", "../fork.git", "beep/fix"]);
    git(&work, &["checkout", "main"]);
    git(&work, &["branch", "-D", "beep/fix"]);
    git(&work, &["remote", "add", "fork", "../fork.git"]);
    git(&work, &["branch", "local-only"]);

    let folder = work.to_str().expect("a UTF-8 path");
    assert_eq!(sh(&add_worktree_script(folder, "beep/fix")), Some(0));
    assert!(work.join(".worktrees/beep-fix/README.md").exists());
    // Kept out of the checkout's status.
    let status = Command::new("git")
        .arg("-C")
        .arg(&work)
        .args(["status", "--porcelain"])
        .output()
        .expect("git runs");
    assert!(status.stdout.is_empty(), "{status:?}");
    assert_eq!(sh(&add_worktree_script(folder, "beep/fix")), Some(5));
    assert_eq!(sh(&add_worktree_script(folder, "local-only")), Some(0));
    assert!(work.join(".worktrees/local-only").exists());
    assert_eq!(sh(&add_worktree_script(folder, "nowhere")), Some(7));
    assert_eq!(
        sh(&add_worktree_script("/no/such/folder", "beep/fix")),
        Some(3)
    );
}

#[test]
fn branches_join_local_and_pushed_and_put_the_default_first() {
    let out = "@@default main\n@@base aaa\n\
            @@ref refs/heads/login-copy\tbbb\t200\t/srv/project/.worktrees/login\tFix the copy\n\
            @@ref refs/remotes/origin/login-copy\tccc\t150\t\tFix the copy\n\
            @@ref refs/remotes/origin/HEAD\taaa\t100\t\t\n\
            @@ref refs/heads/main\taaa\t100\t/srv/project\tRelease\n\
            @@ref refs/remotes/origin/spike\tddd\t300\t\tTry\twidgets\n\
            @@ab bbb 1\t4\n@@ab aaa 0\t0\n@@ab ddd 7\t2\n";
    let (default, branches) = parse_branches(out).unwrap();
    assert_eq!(default, "main");
    let names: Vec<&str> = branches.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["main", "spike", "login-copy"]);
    let login = &branches[2];
    assert!(login.local && login.pushed);
    assert_eq!(login.head, "bbb");
    assert_eq!((login.ahead, login.behind), (4, 1));
    assert_eq!(
        login.worktree.as_deref(),
        Some("/srv/project/.worktrees/login")
    );
    let spike = &branches[1];
    assert!(!spike.local && spike.pushed);
    assert_eq!(spike.subject, "Try\twidgets");
    assert_eq!(spike.worktree, None);
    assert!(parse_branches("").is_err());
}

#[test]
fn branches_are_listed_for_a_checkout_with_spaces_and_an_apostrophe() {
    let tmp = TempDir::new().expect("a temp dir");
    let folder = tmp.path().join("Pat's checkout");
    fs::create_dir(&folder).expect("a checkout folder");
    git(&folder, &["init", "-b", "main"]);
    git(&folder, &["commit", "--allow-empty", "-m", "Initial"]);
    git(&folder, &["branch", "release/one"]);

    let script = branches_script(folder.to_str().expect("a UTF-8 path"), "release/one");
    let output = Command::new("sh")
        .args(["-c", &script])
        .output()
        .expect("sh runs");
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).expect("branch listing is UTF-8");
    let (default, branches) = parse_branches(&text).expect("the branch listing parses");
    assert_eq!(default, "release/one");
    let names: Vec<_> = branches.iter().map(|branch| branch.name.as_str()).collect();
    assert_eq!(names, ["release/one", "main"]);
}
