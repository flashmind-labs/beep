//! A project's branches with work, and giving one a worktree.

use crate::{
    NO_GIT_SH, Shell, ShellOutput, commands::exclude_project_dirs, github_owner_var,
    model::MachinePath, quote,
};
use anyhow::{Result, bail};
use beep_proto::{ProjectBranch, WorkBranch, node::agent};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// How long adding a worktree may take, fetch included.
const ADD_LIMIT: Duration = Duration::from_secs(120);
/// How long listing branches or pull requests may take.
pub const READ_LIMIT: Duration = Duration::from_secs(30);
/// The most open pull requests read.
const MAX_PRS: u32 = 50;

/// The branch's worktree path: `.worktrees/` plus the name with `/` as `-`.
pub fn worktree_path(branch: &str) -> String {
    agent::worktree_path(branch)
}

/// Gives `branch` a worktree in `folder`, from the local branch or the first remote that has it.
/// Exits 3 no folder, 4 git failed, 5 worktree exists, 6 path taken, 7 no remote has it.
pub fn add_worktree_script(folder: &str, branch: &str) -> String {
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
pub fn tail(text: &str) -> &str {
    let text = text.trim();
    let mut start = text.len().saturating_sub(300);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

/// Adds `branch`'s worktree on `node` and gives the branch as git lists it after.
pub async fn add_worktree(
    shell: &dyn Shell,
    machine: &str,
    folder: &str,
    branch: &str,
    default_branch: &str,
) -> Result<ProjectBranch, String> {
    let ran = shell
        .run(&add_worktree_script(folder, branch), ADD_LIMIT)
        .await
        .map_err(|e| format!("{machine}: {e:#}"))?;
    if ran.exit_code != Some(0) {
        return Err(add_failure(ran.exit_code, branch, &ran.output));
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
    match read_branches(shell, folder, default_branch).await {
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

/// The checkout's default branch and its branches, for `ListProjectBranches`.
pub async fn read_branches(
    shell: &dyn Shell,
    folder: &str,
    default_branch: &str,
) -> Result<(String, Vec<ProjectBranch>)> {
    parse_branches(&run_script(shell, &branches_script(folder, default_branch), READ_LIMIT).await?)
}

/// Runs `script` on `node` and gives its output; fails unless it exits 0.
pub async fn run_script(node: &dyn Shell, script: &str, limit: Duration) -> Result<String> {
    let ran = node.run(script, limit).await?;
    ok(&ran)?;
    Ok(ran.output)
}

fn ok(ran: &ShellOutput) -> Result<()> {
    match ran.exit_code {
        Some(0) => Ok(()),
        Some(3) => bail!("the project's folder is not there"),
        Some(NO_GIT_SH) => bail!("{}", tail(&ran.output)),
        Some(code) => bail!("exited with {code}: {}", tail(&ran.output)),
        None => bail!("did not finish in time"),
    }
}

/// The most branches listed.
pub const MAX_BRANCHES: usize = 100;
/// The most refs read for a list.
const MAX_REFS: u32 = 300;
/// Sets `$d` to the default branch (`origin`'s HEAD, else `fallback`) and `$base` to its commit.
pub fn base_lines(fallback: &str) -> String {
    format!(
        "d=$(git symbolic-ref -q --short refs/remotes/origin/HEAD 2>/dev/null | sed 's#^origin/##'); \
         [ -n \"$d\" ] || d={fallback}; \
         base=$(git rev-parse --verify -q \"refs/remotes/origin/$d^{{commit}}\" || git rev-parse --verify -q \"refs/heads/$d^{{commit}}\"); \
         echo \"@@default $d\"; echo \"@@base $base\"; ",
        fallback = quote(fallback),
    )
}

/// Lists branches with last commit, worktree and distance from the default branch. Read-only.
pub fn branches_script(folder: &str, default: &str) -> String {
    let refs = "refs/heads refs/remotes/origin";
    format!(
        "cd -- {folder} || exit 3; {base}\
         git for-each-ref --sort=-committerdate --count={MAX_REFS} \
           --format='@@ref %(refname)%09%(objectname)%09%(committerdate:unix)%09%(worktreepath)%09%(contents:subject)' {refs} || exit 4; \
         if [ -n \"$base\" ]; then \
           git for-each-ref --sort=-committerdate --count={MAX_REFS} --format='%(objectname)' {refs} | sort -u | \
           while read -r sha; do echo \"@@ab $sha $(git rev-list --left-right --count \"$base...$sha\")\"; done; \
         fi",
        folder = quote(folder),
        base = base_lines(default),
    )
}

/// One branch as the output names it, before local and remote are joined.
struct Ref {
    name: String,
    local: bool,
    head: String,
    at: u64,
    worktree: Option<String>,
    subject: String,
}

pub fn at(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

/// Parses `branches_script`: default first then newest, and the default branch's name.
pub fn parse_branches(output: &str) -> Result<(String, Vec<ProjectBranch>)> {
    let mut default = None;
    let mut refs = Vec::new();
    // Behind, then ahead, by commit.
    let mut distance: HashMap<&str, (u32, u32)> = HashMap::new();
    for line in output.lines() {
        if let Some(name) = line.strip_prefix("@@default ") {
            default = Some(name.trim().to_owned());
        } else if let Some(rest) = line.strip_prefix("@@ref ") {
            let mut parts = rest.splitn(5, '\t');
            let (Some(name), Some(head), Some(secs), Some(worktree), subject) = (
                parts.next(),
                parts.next(),
                parts.next(),
                parts.next(),
                parts.next(),
            ) else {
                continue;
            };
            let (name, local) = if let Some(name) = name.strip_prefix("refs/heads/") {
                (name, true)
            } else if let Some(name) = name.strip_prefix("refs/remotes/origin/") {
                (name, false)
            } else {
                continue;
            };
            if name == "HEAD" {
                continue;
            }
            refs.push(Ref {
                name: name.to_owned(),
                local,
                head: head.to_owned(),
                at: secs.trim().parse().unwrap_or(0),
                worktree: Some(worktree.trim())
                    .filter(|path| !path.is_empty())
                    .map(str::to_owned),
                subject: subject.unwrap_or_default().trim().to_owned(),
            });
        } else if let Some(rest) = line.strip_prefix("@@ab ") {
            let mut parts = rest.split_whitespace();
            if let (Some(sha), Some(behind), Some(ahead)) =
                (parts.next(), parts.next(), parts.next())
                && let (Ok(behind), Ok(ahead)) = (behind.parse(), ahead.parse())
            {
                distance.insert(sha, (behind, ahead));
            }
        }
    }
    let Some(default) = default else {
        bail!("git did not answer as expected");
    };
    let mut joined: BTreeMap<String, ProjectBranch> = BTreeMap::new();
    for one in refs {
        let (behind, ahead) = distance.get(one.head.as_str()).copied().unwrap_or((0, 0));
        let fresh = ProjectBranch {
            name: one.name.clone(),
            worktree: one.worktree,
            ahead,
            behind,
            head: one.head,
            subject: one.subject,
            committed_at: at(one.at),
            local: one.local,
            pushed: !one.local,
        };
        match joined.get_mut(&one.name) {
            None => {
                joined.insert(one.name, fresh);
            }
            // The local branch speaks for both.
            Some(kept) if one.local => {
                *kept = ProjectBranch {
                    pushed: kept.pushed,
                    ..fresh
                };
            }
            Some(kept) => kept.pushed = true,
        }
    }
    let mut branches: Vec<ProjectBranch> = joined.into_values().collect();
    branches.sort_by(|a, b| {
        (b.name == default)
            .cmp(&(a.name == default))
            .then(b.committed_at.cmp(&a.committed_at))
            .then(a.name.cmp(&b.name))
    });
    branches.truncate(MAX_BRANCHES);
    Ok((default, branches))
}

/// One of the user's open pull requests, by the branch it is from.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OpenPr {
    pub number: u64,
    #[serde(default)]
    pub title: String,
    #[serde(rename = "headRefName")]
    pub head: String,
}

/// Lists the user's open pull requests on GitHub `repo` with `gh`, as the owner's account.
pub fn my_prs_command(repo: &str) -> String {
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

pub fn parse_prs(out: &str) -> Result<Vec<OpenPr>> {
    Ok(serde_json::from_str(out.trim())?)
}

/// Reads the user's open pull requests on `node`.
pub async fn my_prs(shell: &dyn Shell, repo: &str) -> Result<Vec<OpenPr>> {
    parse_prs(&run_script(shell, &my_prs_command(repo), READ_LIMIT).await?)
}

/// Branches with work (open PR, ahead of default, or with a worktree), then PRs whose branch
/// is missing locally, then the default branch.
pub fn branches_with_work(
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
    fn the_branches_script_quotes_what_it_is_given() {
        assert!(branches_script("/srv/project", "two words").contains("d='two words'"));
    }
}
