//! Git scripts for a project's branches and one branch's diff, and parsing their output.

use super::super::commands::quote;
use crate::protocol::ProjectBranch;
use anyhow::{Result, bail};
use std::{
    collections::{BTreeMap, HashMap},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// The most branches listed.
pub(crate) const MAX_BRANCHES: usize = 100;
/// The most refs read for a list.
const MAX_REFS: u32 = 300;
/// The most changed files read for a review.
const MAX_FILES: u32 = 2000;
/// The most of the head's `.gitattributes` read, in bytes.
const MAX_ATTRIBUTES: u32 = 64 * 1024;

/// Sets `$d` to the default branch (`origin`'s HEAD, else `fallback`) and `$base` to its commit.
fn base_lines(fallback: &str) -> String {
    format!(
        "d=$(git symbolic-ref -q --short refs/remotes/origin/HEAD 2>/dev/null | sed 's#^origin/##'); \
         [ -n \"$d\" ] || d={fallback}; \
         base=$(git rev-parse --verify -q \"refs/remotes/origin/$d^{{commit}}\" || git rev-parse --verify -q \"refs/heads/$d^{{commit}}\"); \
         echo \"@@default $d\"; echo \"@@base $base\"; ",
        fallback = quote(fallback),
    )
}

/// Lists branches with last commit, worktree and distance from the default branch. Read-only.
pub(crate) fn branches_script(folder: &str, default: &str) -> String {
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

fn at(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

/// Parses `branches_script`: default first then newest, and the default branch's name.
pub(crate) fn parse_branches(output: &str) -> Result<(String, Vec<ProjectBranch>)> {
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

/// Fetches, then prints `branch`'s head, merge base, mergeability and changed files. Read-only.
pub(crate) fn review_script(folder: &str, branch: &str, default: &str) -> String {
    let branch = quote(branch);
    format!(
        "cd -- {folder} || exit 3; git fetch --quiet origin >/dev/null 2>&1; {base}\
         [ -n \"$base\" ] || {{ echo @@nobase; exit 0; }}; \
         b={branch}; \
         head=$(git rev-parse --verify -q \"refs/heads/$b^{{commit}}\" || git rev-parse --verify -q \"refs/remotes/origin/$b^{{commit}}\") \
           || {{ echo @@nobranch; exit 0; }}; \
         mb=$(git merge-base \"$base\" \"$head\") || {{ echo @@nobase; exit 0; }}; \
         echo \"@@head $head $(git log -1 --format=%ct \"$head\")\"; \
         echo \"@@mergebase $mb\"; \
         echo @@merge; git merge-tree --write-tree --name-only --no-messages \"$base\" \"$head\" 2>/dev/null; \
         echo \"@@merged $?\"; \
         echo @@attrs; git show \"$head:.gitattributes\" 2>/dev/null | head -c {MAX_ATTRIBUTES}; echo; \
         echo @@numstat; git -c core.quotePath=false diff --numstat --no-renames \"$mb\" \"$head\" | head -n {MAX_FILES}",
        folder = quote(folder),
        base = base_lines(default),
    )
}

/// One changed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Changed {
    pub(crate) path: String,
    /// Added and removed; 0 for a binary file.
    pub(crate) lines: u32,
}

/// What `review_script` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Facts {
    pub(crate) default: String,
    pub(crate) head: String,
    pub(crate) head_at: SystemTime,
    pub(crate) merge_base: String,
    /// `None` when git could not say.
    pub(crate) conflicts: Option<Vec<String>>,
    pub(crate) changed: Vec<Changed>,
    /// The head's `.gitattributes`, empty when it has none.
    pub(crate) attributes: String,
}

/// Why `review_script` found nothing to review.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum FactsError {
    #[error("The branch is not in the checkout or on origin.")]
    NoBranch,
    #[error("The default branch {0} is not in the checkout, or shares no history with it.")]
    NoBase(String),
    #[error("git did not answer as expected.")]
    Garbled,
}

pub(crate) fn parse_facts(output: &str) -> Result<Facts, FactsError> {
    let mut default = String::new();
    let mut head = None;
    let mut merge_base = None;
    let mut merge_lines: Option<Vec<String>> = None;
    let mut merged = None;
    let mut changed = Vec::new();
    let mut in_numstat = false;
    let mut attributes: Option<Vec<&str>> = None;
    for line in output.lines() {
        if line == "@@nobranch" {
            return Err(FactsError::NoBranch);
        } else if line == "@@nobase" {
            return Err(FactsError::NoBase(default));
        } else if let Some(name) = line.strip_prefix("@@default ") {
            default = name.trim().to_owned();
        } else if let Some(rest) = line.strip_prefix("@@head ") {
            let mut parts = rest.split_whitespace();
            let sha = parts.next().unwrap_or_default().to_owned();
            let secs: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            head = Some((sha, at(secs)));
        } else if let Some(sha) = line.strip_prefix("@@mergebase ") {
            merge_base = Some(sha.trim().to_owned());
        } else if line == "@@merge" {
            merge_lines = Some(Vec::new());
        } else if let Some(code) = line.strip_prefix("@@merged ") {
            merged = code.trim().parse::<i32>().ok();
        } else if line == "@@attrs" {
            attributes = Some(Vec::new());
        } else if line == "@@numstat" {
            in_numstat = true;
        } else if in_numstat {
            let mut parts = line.splitn(3, '\t');
            if let (Some(added), Some(removed), Some(path)) =
                (parts.next(), parts.next(), parts.next())
            {
                let lines = added
                    .parse::<u32>()
                    .unwrap_or(0)
                    .saturating_add(removed.parse::<u32>().unwrap_or(0));
                changed.push(Changed {
                    path: path.to_owned(),
                    lines,
                });
            }
        } else if let Some(lines) = attributes.as_mut() {
            lines.push(line);
        } else if merged.is_none()
            && let Some(lines) = merge_lines.as_mut()
            && !line.trim().is_empty()
        {
            lines.push(line.trim().to_owned());
        }
    }
    let (Some((head, head_at)), Some(merge_base)) = (head, merge_base) else {
        return Err(FactsError::Garbled);
    };
    // The first line is the merged tree; conflicted files follow it.
    let conflicts = match merged {
        Some(0) => Some(Vec::new()),
        Some(1) => Some(
            merge_lines
                .unwrap_or_default()
                .into_iter()
                .skip(1)
                .collect(),
        ),
        _ => None,
    };
    Ok(Facts {
        default,
        head,
        head_at,
        merge_base,
        conflicts,
        changed,
        attributes: attributes.unwrap_or_default().join("\n"),
    })
}

/// Fetches `branch`, prints its local and `origin` heads and whether `origin` is behind `reviewed`.
pub(crate) fn head_script(folder: &str, branch: &str, reviewed: &str) -> String {
    format!(
        "cd -- {folder} || exit 3; b={branch}; r={reviewed}; \
         git fetch --quiet origin \"$b\" >/dev/null 2>&1; \
         echo \"@@local $(git rev-parse --verify -q \"refs/heads/$b^{{commit}}\")\"; \
         o=$(git rev-parse --verify -q \"refs/remotes/origin/$b^{{commit}}\"); echo \"@@remote $o\"; \
         if [ -n \"$o\" ] && git merge-base --is-ancestor \"$o\" \"$r\" 2>/dev/null; then echo @@behind; fi",
        folder = quote(folder),
        branch = quote(branch),
        reviewed = quote(reviewed),
    )
}

/// Where a reviewed branch is now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HeadNow {
    /// The local branch's head, else `origin`'s; `None` when it is gone.
    pub(crate) head: Option<String>,
    /// Moved since the review: local head differs, or `origin` has unread commits.
    pub(crate) stale: bool,
}

/// Reads what `head_script` printed about a branch reviewed at `reviewed`.
pub(crate) fn parse_head(output: &str, reviewed: &str) -> HeadNow {
    let sha = |rest: &str| Some(rest.trim().to_owned()).filter(|sha| !sha.is_empty());
    let mut local = None;
    let mut remote = None;
    let mut behind = false;
    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("@@local") {
            local = sha(rest);
        } else if let Some(rest) = line.strip_prefix("@@remote") {
            remote = sha(rest);
        } else if line.trim() == "@@behind" {
            behind = true;
        }
    }
    let stale = local.as_deref().is_some_and(|sha| sha != reviewed)
        || remote
            .as_deref()
            .is_some_and(|sha| sha != reviewed && !behind);
    HeadNow {
        head: local.or(remote),
        stale,
    }
}

/// Makes a detached worktree at `head` for reviewers, hidden from the checkout's `git status`.
pub(crate) fn add_review_worktree(folder: &str, worktree: &str, head: &str) -> String {
    format!(
        "cd -- {folder} || exit 3; \
         exclude=\"$(git rev-parse --path-format=absolute --git-common-dir)/info/exclude\" \
         && mkdir -p \"$(dirname \"$exclude\")\" \
         && {{ grep -qxF .worktrees/ \"$exclude\" 2>/dev/null || echo .worktrees/ >> \"$exclude\"; }} \
         && git worktree add --quiet --detach {worktree} {head}",
        folder = quote(folder),
        worktree = quote(worktree),
        head = quote(head),
    )
}

/// Removes a review's worktree; one already gone is fine.
pub(crate) fn remove_review_worktree(folder: &str, worktree: &str) -> String {
    format!(
        "cd -- {folder} || exit 3; git worktree remove --force {worktree} 2>/dev/null; \
         git worktree prune; exit 0",
        folder = quote(folder),
        worktree = quote(worktree),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changed(path: &str, lines: u32) -> Changed {
        Changed {
            path: path.into(),
            lines,
        }
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
    fn facts_name_the_conflicts_and_the_changed_files() {
        let out = "@@default main\n@@base aaa\n@@head bbb 1700000000\n@@mergebase ccc\n\
            @@merge\ntreeoid\napps/web/copy.ts\nCargo.toml\n@@merged 1\n\
            @@attrs\ngen/** linguist-generated\n\n\
            @@numstat\n10\t2\tapps/web/copy.ts\n-\t-\tlogo.png\n";
        let facts = parse_facts(out).unwrap();
        assert_eq!(facts.head, "bbb");
        assert_eq!(facts.attributes.trim(), "gen/** linguist-generated");
        assert_eq!(facts.merge_base, "ccc");
        assert_eq!(
            facts.conflicts,
            Some(vec!["apps/web/copy.ts".to_owned(), "Cargo.toml".to_owned()])
        );
        assert_eq!(
            facts.changed,
            vec![changed("apps/web/copy.ts", 12), changed("logo.png", 0)]
        );
        let clean = out.replace("@@merged 1", "@@merged 0");
        assert_eq!(parse_facts(&clean).unwrap().conflicts, Some(Vec::new()));
        let old_git = out.replace("@@merged 1", "@@merged 129");
        assert_eq!(parse_facts(&old_git).unwrap().conflicts, None);
        assert_eq!(
            parse_facts("@@default main\n@@nobranch\n"),
            Err(FactsError::NoBranch)
        );
        assert_eq!(
            parse_facts("@@default trunk\n@@nobase\n"),
            Err(FactsError::NoBase("trunk".into()))
        );
    }

    #[test]
    fn review_goes_stale_on_push() {
        let same = "@@local aaa\n@@remote aaa\n@@behind\n";
        assert_eq!(
            parse_head(same, "aaa"),
            HeadNow {
                head: Some("aaa".into()),
                stale: false
            }
        );
        // Reviewed before its local commits were pushed.
        assert!(!parse_head("@@local aaa\n@@remote 999\n@@behind\n", "aaa").stale);
        // Someone pushed on top.
        let pushed = parse_head("@@local \n@@remote bbb\n", "aaa");
        assert_eq!(pushed.head.as_deref(), Some("bbb"));
        assert!(pushed.stale);
        // A new local commit.
        assert!(parse_head("@@local ccc\n@@remote aaa\n@@behind\n", "aaa").stale);
        assert_eq!(parse_head("@@local \n@@remote \n", "aaa").head, None);
    }

    #[test]
    fn scripts_quote_what_they_are_given() {
        let script = review_script("/srv/my project", "x; rm -rf /", "main");
        assert!(script.contains("cd -- '/srv/my project'"), "{script}");
        assert!(script.contains("b='x; rm -rf /'"), "{script}");
        assert!(branches_script("/srv/project", "two words").contains("d='two words'"));
    }
}
