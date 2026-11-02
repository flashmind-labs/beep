//! A project's branches with work, and giving one a worktree: the shell
//! scripts that ask git and the host, and reading what they print.

pub(crate) use super::commands::quote;
use super::{
    commands::{CommandOutput, Shell, exclude_project_dirs},
    model::MachinePath,
    review::git::{branches_script, parse_branches},
};
use crate::{
    credentials::github_owner_var,
    nodes::Online,
    protocol::{ProjectBranch, WorkBranch, node::agent},
};
use anyhow::{Result, bail};
use serde::Deserialize;
use std::time::{Duration, SystemTime};

/// The longest branch name taken.
const MAX_BRANCH: usize = 250;
/// How long adding a worktree may take, fetch included.
const ADD_LIMIT: Duration = Duration::from_secs(120);
/// How long listing branches or pull requests may take.
pub(crate) const READ_LIMIT: Duration = Duration::from_secs(30);
/// The most open pull requests read.
const MAX_PRS: u32 = 50;

/// A branch name as git takes it: one line, not an option, no `..` and
/// none of the characters git refuses in a ref.
pub(crate) fn branch_name(text: &str) -> Option<&str> {
    let text = text.trim();
    let bad = text.is_empty()
        || text.len() > MAX_BRANCH
        || text.starts_with('-')
        || text.starts_with('/')
        || text.ends_with('/')
        || text.ends_with(".lock")
        || text.contains("..")
        || text.contains("//")
        || text.contains("@{")
        || text.contains(|c: char| {
            c.is_control()
                || c.is_whitespace()
                || matches!(c, ':' | '~' | '^' | '?' | '*' | '[' | '\\')
        });
    (!bad).then_some(text)
}

/// Where a branch's worktree goes in its checkout: `.worktrees/` and the
/// name with each `/` as `-`.
pub(crate) fn worktree_path(branch: &str) -> String {
    agent::worktree_path(branch)
}

/// Gives `branch` a worktree at [`worktree_path`] in the checkout
/// `folder`: the local branch when there is one, else fetched from
/// `origin`, then from the other remotes, tracking the one that has it.
/// Exits 3 when the folder is not there, 5 when the branch has a worktree
/// already, 6 when the path is taken, 7 when no remote has the branch and
/// 4 when git fails.
pub(crate) fn add_worktree_script(folder: &str, branch: &str) -> String {
    let exclude = MachinePath::parse(folder)
        .map(|path| format!("{{ {}; }} >/dev/null 2>&1; ", exclude_project_dirs(&path)))
        .unwrap_or_default();
    format!(
        "cd -- {folder} || exit 3; {exclude}b={branch}; p={path}; \
         git worktree list --porcelain | grep -qxF \"branch refs/heads/$b\" && exit 5; \
         [ -e \"$p\" ] && exit 6; \
         if git rev-parse -q --verify \"refs/heads/$b\" >/dev/null; then \
           git worktree add --quiet \"$p\" \"$b\" || exit 4; \
         else \
           r=; for x in origin $(git remote | grep -vx origin); do \
             if git fetch --quiet \"$x\" \"+refs/heads/$b:refs/remotes/$x/$b\" 2>/dev/null; then r=$x; break; fi; \
           done; \
           [ -n \"$r\" ] || exit 7; \
           git worktree add --quiet --track -b \"$b\" \"$p\" \"$r/$b\" || exit 4; \
         fi",
        folder = quote(folder),
        branch = quote(branch),
        path = quote(&worktree_path(branch)),
    )
}

/// Why a worktree was not added, in words for the user.
fn add_failure(code: Option<i32>, branch: &str, out: &str) -> String {
    match code {
        Some(3) => "The project's folder is not there.".to_owned(),
        Some(5) => format!("{branch} already has a worktree there."),
        Some(6) => format!("{} is taken in the checkout.", worktree_path(branch)),
        Some(7) => format!("{branch} is not in the checkout or on any of its remotes."),
        None => "git did not finish in time.".to_owned(),
        Some(_) => format!("git failed: {}", tail(out)),
    }
}

/// The end of `text`, cut on a character boundary.
fn tail(text: &str) -> &str {
    let text = text.trim();
    let mut start = text.len().saturating_sub(300);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

/// Adds `branch`'s worktree in `folder` on `node` and gives the branch as
/// git lists it after. The error is for the user.
pub(crate) async fn add_worktree(
    node: &Online,
    folder: &str,
    branch: &str,
    default_branch: &str,
) -> Result<ProjectBranch, String> {
    let ran = node
        .run(&add_worktree_script(folder, branch), ADD_LIMIT)
        .await
        .map_err(|e| format!("{}: {e:#}", node.name))?;
    if ran.code != Some(0) {
        return Err(add_failure(ran.code, branch, &ran.out));
    }
    let fallback = || ProjectBranch {
        name: branch.to_owned(),
        worktree: Some(format!(
            "{}/{}",
            folder.trim_end_matches('/'),
            worktree_path(branch)
        )),
        ahead: 0,
        behind: 0,
        head: String::new(),
        subject: String::new(),
        committed_at: SystemTime::now(),
        local: true,
        pushed: false,
    };
    // The worktree is in; a list that fails still says so.
    match read_branches(node, folder, default_branch).await {
        Ok((_, branches)) => Ok(branches
            .into_iter()
            .find(|one| one.name == branch)
            .unwrap_or_else(fallback)),
        Err(e) => {
            log::info!("branches: not read after adding a worktree: {e:#}");
            Ok(fallback())
        }
    }
}

/// The checkout's default branch and its branches, as `ListProjectBranches`
/// reads them.
pub(crate) async fn read_branches(
    node: &Online,
    folder: &str,
    default_branch: &str,
) -> Result<(String, Vec<ProjectBranch>)> {
    parse_branches(&run_script(node, &branches_script(folder, default_branch), READ_LIMIT).await?)
}

/// Runs `script` on `node` and gives what it printed, failing unless it
/// exits 0.
pub(crate) async fn run_script(node: &Online, script: &str, limit: Duration) -> Result<String> {
    let ran = node.run(script, limit).await?;
    ok(&ran)?;
    Ok(ran.out)
}

fn ok(ran: &CommandOutput) -> Result<()> {
    match ran.code {
        Some(0) => Ok(()),
        Some(3) => bail!("the project's folder is not there"),
        Some(code) => bail!("exited with {code}: {}", tail(&ran.out)),
        None => bail!("did not finish in time"),
    }
}

/// One of the user's open pull requests, by the branch it is from.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct OpenPr {
    pub(crate) number: u64,
    #[serde(default)]
    pub(crate) title: String,
    #[serde(rename = "headRefName")]
    pub(crate) head: String,
}

/// Lists the user's open pull requests on GitHub's `repo` (`org/repo`)
/// with `gh`, as the account for the repository's owner.
pub(crate) fn my_prs_command(repo: &str) -> String {
    let account = repo
        .split_once('/')
        .and_then(|(owner, _)| github_owner_var(owner))
        .map(|var| format!("GH_TOKEN=\"${{{var}:-$GH_TOKEN}}\" "))
        .unwrap_or_default();
    format!(
        "{account}gh pr list -R {repo} --author @me --state open --limit {MAX_PRS} --json number,title,headRefName",
        repo = quote(repo),
    )
}

pub(crate) fn parse_prs(out: &str) -> Result<Vec<OpenPr>> {
    Ok(serde_json::from_str(out.trim())?)
}

/// Reads the user's open pull requests on `node`.
pub(crate) async fn my_prs(node: &Online, repo: &str) -> Result<Vec<OpenPr>> {
    parse_prs(&run_script(node, &my_prs_command(repo), READ_LIMIT).await?)
}

/// The branches with work: those an open pull request is from, those
/// ahead of `default_branch` and those with a worktree, in the order
/// given; then the pull requests whose branch the checkout does not have;
/// then the default branch.
pub(crate) fn branches_with_work(
    branches: &[ProjectBranch],
    prs: &[OpenPr],
    default_branch: &str,
) -> Vec<WorkBranch> {
    let pr_of = |name: &str| prs.iter().find(|pr| pr.head == name);
    let shown = |branch: &ProjectBranch| {
        let pr = pr_of(&branch.name);
        WorkBranch {
            name: branch.name.clone(),
            pr: pr.map(|pr| pr.number),
            pr_title: pr.map(|pr| pr.title.clone()).unwrap_or_default(),
            ahead: branch.ahead,
            worktree: branch.worktree.clone(),
            in_checkout: true,
            committed_at: Some(branch.committed_at),
        }
    };
    let mut out: Vec<WorkBranch> = branches
        .iter()
        .filter(|branch| branch.name != default_branch)
        .filter(|branch| {
            pr_of(&branch.name).is_some() || branch.ahead > 0 || branch.worktree.is_some()
        })
        .map(shown)
        .collect();
    for pr in prs {
        if pr.head != default_branch && !branches.iter().any(|branch| branch.name == pr.head) {
            out.push(WorkBranch {
                name: pr.head.clone(),
                pr: Some(pr.number),
                pr_title: pr.title.clone(),
                ahead: 0,
                worktree: None,
                in_checkout: false,
                committed_at: None,
            });
        }
    }
    out.extend(
        branches
            .iter()
            .filter(|branch| branch.name == default_branch)
            .map(shown),
    );
    out
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn worktree_path_flattens_slashes() {
        assert_eq!(worktree_path("login-copy"), ".worktrees/login-copy");
        assert_eq!(
            worktree_path("beep/fix/android"),
            ".worktrees/beep-fix-android"
        );
    }

    #[test]
    fn branch_names_git_refuses_are_refused() {
        assert_eq!(branch_name(" beep/login "), Some("beep/login"));
        for bad in [
            "", "-x", "a b", "a..b", "a:b", "a~1", "/a", "a/", "x.lock", "a\nb",
        ] {
            assert_eq!(branch_name(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn prs_read_from_gh() {
        let out = r#"[{"number":41,"title":"Fix the copy","headRefName":"login-copy"}]"#;
        assert_eq!(
            parse_prs(out).unwrap(),
            [OpenPr {
                number: 41,
                title: "Fix the copy".into(),
                head: "login-copy".into(),
            }]
        );
        assert!(my_prs_command("acme/widget").contains("gh pr list -R acme/widget --author @me"));
        assert!(parse_prs("not json").is_err());
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
}
