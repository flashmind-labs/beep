//! What git on a machine says about a project's branches and one branch's
//! diff: the shell scripts that ask, and reading their output.

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
/// The most shards a review has; the smallest are put together past it.
pub(crate) const MAX_SHARDS: usize = 6;
/// A top-level folder with more changed lines than this is split by its
/// subfolders.
const SPLIT_LINES: u32 = 1200;

/// The lines that find the default branch (`origin`'s HEAD, else
/// `fallback`) into `$d` and its commit into `$base`.
fn base_lines(fallback: &str) -> String {
    format!(
        "d=$(git symbolic-ref -q --short refs/remotes/origin/HEAD 2>/dev/null | sed 's#^origin/##'); \
         [ -n \"$d\" ] || d={fallback}; \
         base=$(git rev-parse --verify -q \"refs/remotes/origin/$d^{{commit}}\" || git rev-parse --verify -q \"refs/heads/$d^{{commit}}\"); \
         echo \"@@default $d\"; echo \"@@base $base\"; ",
        fallback = quote(fallback),
    )
}

/// Lists the checkout's branches with their last commit and worktree,
/// then how far each commit is from the default branch. Reads only.
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

/// The branches `branches_script` printed, the default first, then newest
/// commit first, and the default branch's name.
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

/// Fetches, then prints what a review needs of `branch` against the
/// default branch: its head, where it forked, whether it merges, and the
/// changed files. Changes nothing in the checkout.
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
    })
}

/// One reviewer's part of the diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Shard {
    pub(crate) area: String,
    pub(crate) paths: Vec<String>,
    pub(crate) lines: u32,
}

/// The folders of `path`, without the file.
fn dirs(path: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = path.split('/').collect();
    parts.pop();
    parts
}

/// The folders all `paths` share, at most two deep, or `.` for none.
fn area_of(paths: &[String]) -> String {
    let mut common: Option<Vec<&str>> = None;
    for path in paths {
        let here = dirs(path);
        common = Some(match common {
            None => here,
            Some(kept) => kept
                .iter()
                .zip(&here)
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| *a)
                .collect(),
        });
    }
    let common: Vec<&str> = common.unwrap_or_default().into_iter().take(2).collect();
    if common.is_empty() {
        ".".to_owned()
    } else {
        common.join("/")
    }
}

fn shard_of(files: &[&Changed]) -> Shard {
    let paths: Vec<String> = files.iter().map(|file| file.path.clone()).collect();
    Shard {
        area: area_of(&paths),
        lines: files.iter().map(|file| file.lines).sum(),
        paths,
    }
}

/// Splits the changed files by top-level folder (the files at the top are
/// one shard); a big folder is split by its subfolders. Past
/// `MAX_SHARDS`, the smallest are put together. Biggest first.
pub(crate) fn shards(changed: &[Changed]) -> Vec<Shard> {
    let mut top: BTreeMap<&str, Vec<&Changed>> = BTreeMap::new();
    for file in changed {
        let key = match file.path.split_once('/') {
            Some((first, _)) => first,
            None => ".",
        };
        top.entry(key).or_default().push(file);
    }
    let mut out = Vec::new();
    for (key, files) in top {
        let lines: u32 = files.iter().map(|file| file.lines).sum();
        let mut sub: BTreeMap<&str, Vec<&Changed>> = BTreeMap::new();
        for file in &files {
            let second = file
                .path
                .split('/')
                .nth(1)
                .filter(|_| dirs(&file.path).len() >= 2)
                .unwrap_or("");
            sub.entry(second).or_default().push(file);
        }
        if key != "." && lines > SPLIT_LINES && sub.len() > 1 {
            out.extend(sub.values().map(|files| shard_of(files)));
        } else {
            out.push(shard_of(&files));
        }
    }
    out.sort_by(|a, b| b.lines.cmp(&a.lines).then(a.area.cmp(&b.area)));
    if out.len() > MAX_SHARDS {
        let rest = out.split_off(MAX_SHARDS - 1);
        let names: Vec<&str> = rest.iter().map(|shard| shard.area.as_str()).collect();
        let area = format!("{} and {} more", names[0], names.len() - 1);
        out.push(Shard {
            area,
            lines: rest.iter().map(|shard| shard.lines).sum(),
            paths: rest.into_iter().flat_map(|shard| shard.paths).collect(),
        });
    }
    out
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
            @@numstat\n10\t2\tapps/web/copy.ts\n-\t-\tlogo.png\n";
        let facts = parse_facts(out).unwrap();
        assert_eq!(facts.head, "bbb");
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
    fn shards_follow_top_level_areas() {
        let files = [
            changed("apps/web/src/Login.tsx", 200),
            changed("apps/web/src/copy.ts", 68),
            changed("crates/core/src/auth/limit.rs", 144),
            changed("README.md", 3),
        ];
        let got = shards(&files);
        let areas: Vec<(&str, u32)> = got.iter().map(|s| (s.area.as_str(), s.lines)).collect();
        // Named at most two folders deep.
        assert_eq!(areas, [("apps/web", 268), ("crates/core", 144), (".", 3)]);
    }

    #[test]
    fn a_big_area_splits_and_many_small_ones_join() {
        let big = [
            changed("crates/core/a.rs", 900),
            changed("crates/api/b.rs", 700),
        ];
        let areas: Vec<String> = shards(&big).into_iter().map(|s| s.area).collect();
        assert_eq!(areas, ["crates/core", "crates/api"]);

        let many: Vec<Changed> = (0..9)
            .map(|n| changed(&format!("dir{n}/file.rs"), 10 + n))
            .collect();
        let got = shards(&many);
        assert_eq!(got.len(), MAX_SHARDS);
        let last = got.last().unwrap();
        assert_eq!(last.area, "dir3 and 3 more");
        assert_eq!(last.paths.len(), 4);
        assert_eq!(got.iter().map(|s| s.paths.len()).sum::<usize>(), 9);
    }

    #[test]
    fn scripts_quote_what_they_are_given() {
        let script = review_script("/srv/my project", "x; rm -rf /", "main");
        assert!(script.contains("cd -- '/srv/my project'"), "{script}");
        assert!(script.contains("b='x; rm -rf /'"), "{script}");
        assert!(branches_script("/srv/project", "two words").contains("d='two words'"));
    }
}
