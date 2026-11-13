//! Read-only Git history commands, framed so clipped shell output is rejected.

use crate::{Shell, branches::tail, quote};
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Serialize;
use std::{collections::BTreeMap, str::FromStr, time::Duration};

pub const PAGE_SIZE: usize = 50;
const READ_LIMIT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct CommitId(String);

impl FromStr for CommitId {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        ensure!(
            matches!(value.len(), 40 | 64) && value.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid commit ID"
        );
        Ok(Self(value.to_ascii_lowercase()))
    }
}

impl AsRef<str> for CommitId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Commit {
    pub id: CommitId,
    pub parents: Vec<CommitId>,
    pub author: String,
    pub committed_at: u64,
    pub subject: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefKind {
    Local,
    Remote,
    Tag,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitRef {
    pub name: String,
    pub head: CommitId,
    pub kind: RefKind,
}

#[derive(Debug, Clone, Serialize)]
pub struct Worktree {
    pub path: String,
    pub head: Option<CommitId>,
    pub branch: Option<String>,
    pub main: bool,
}

#[derive(Debug, Serialize)]
pub struct History {
    pub root: String,
    pub refs: Vec<GitRef>,
    pub worktrees: Vec<Worktree>,
    pub commits: Vec<Commit>,
    pub next: Option<u32>,
}

pub struct HistoryAsk<'a> {
    pub folder: &'a str,
    pub revision: Option<&'a CommitId>,
    pub offset: u32,
    pub search: &'a str,
}

fn capture(label: &str, command: &str) -> String {
    format!(
        "{command} >\"$tmp/part\" || exit 4; [ \"$(wc -c <\"$tmp/part\")\" -le 4194304 ] || {{ printf 'Git output is too large. Select an individual file or narrow the history.\\n'; exit 5; }}; printf '@@{label} %s\\n' \"$(wc -c <\"$tmp/part\")\"; base64 <\"$tmp/part\"; printf '\\n'; "
    )
}

fn framed(folder: &str, parts: &[(&str, String)]) -> String {
    format!(
        "cd -- {} || exit 3; tmp=$(mktemp -d) || exit 4; trap 'rm -rf -- \"$tmp\"' EXIT HUP INT TERM; {}printf '@@end\\n'",
        quote(folder),
        parts
            .iter()
            .map(|(label, command)| capture(label, command))
            .collect::<String>()
    )
}

pub fn history_script(ask: &HistoryAsk<'_>) -> String {
    let revision = ask
        .revision
        .map(|id| quote(id.as_ref()))
        .unwrap_or_else(|| "--all".to_owned());
    let search = if ask.search.is_empty() {
        String::new()
    } else {
        format!(
            " --regexp-ignore-case --fixed-strings {}",
            quote(&format!("--grep={}", ask.search))
        )
    };
    let log = format!(
        "git log --date-order --max-count={} --skip={} --format='%H%x00%P%x00%an%x00%ct%x00%s%x00' {search} {revision} --",
        PAGE_SIZE + 1,
        ask.offset
    );
    framed(ask.folder, &[
        ("root", "git rev-parse --show-toplevel".to_owned()),
        ("refs", "git for-each-ref --format='%(refname)%00%(objectname)%00%(symref)%00' refs/heads refs/remotes refs/tags".to_owned()),
        ("worktrees", "git worktree list --porcelain -z".to_owned()),
        ("commits", log),
    ])
}

/// Git's own patch format, including initial commits and a merge's first-parent diff.
pub fn patch_context_script(
    folder: &str,
    commit: &CommitId,
    paths: &[String],
    context: u32,
) -> String {
    let context = context.clamp(3, 50_000);
    let paths = paths.iter().map(|p| quote(p)).collect::<Vec<_>>().join(" ");
    framed(
        folder,
        &[
            ("root", "git rev-parse --show-toplevel".to_owned()),
            (
                "parents",
                format!("git rev-list --parents --no-walk {}", commit.as_ref()),
            ),
            (
                "patch",
                format!(
                    "git --literal-pathspecs -c core.quotepath=off show --unified={context} --format= --root --diff-merges=first-parent --no-ext-diff --no-textconv --no-color --find-renames {} -- {paths}",
                    commit.as_ref()
                ),
            ),
        ],
    )
}

pub async fn run(shell: &dyn Shell, script: &str) -> Result<BTreeMap<String, String>> {
    let ran = shell.run(script, READ_LIMIT).await?;
    ensure!(
        !ran.timed_out && ran.exit_code.is_some(),
        "Git did not finish in time"
    );
    ensure!(
        ran.exit_code == Some(0),
        "Git failed: {}",
        tail(&ran.output)
    );
    decode(&ran.output)
}

fn decode(output: &str) -> Result<BTreeMap<String, String>> {
    let mut result = BTreeMap::new();
    let mut header: Option<(&str, usize)> = None;
    let mut encoded = String::new();
    let flush = |header: Option<(&str, usize)>,
                 encoded: &mut String,
                 result: &mut BTreeMap<String, String>|
     -> Result<()> {
        if let Some((name, bytes)) = header {
            let raw = STANDARD.decode(encoded.as_bytes()).context(
                "Git output was clipped. Select an individual file or narrow the history.",
            )?;
            ensure!(
                raw.len() == bytes,
                "Git output was clipped. Select an individual file or narrow the history."
            );
            result.insert(name.to_owned(), String::from_utf8_lossy(&raw).into_owned());
            encoded.clear();
        }

        Ok(())
    };

    for line in output.lines() {
        if line == "@@end" {
            flush(header.take(), &mut encoded, &mut result)?;
            return Ok(result);
        }

        if let Some(rest) = line.strip_prefix("@@") {
            flush(header.take(), &mut encoded, &mut result)?;
            let (name, bytes) = rest.split_once(' ').context("Invalid Git response")?;
            header = Some((name, bytes.trim().parse()?));
        } else if !line.trim().is_empty() {
            ensure!(header.is_some(), "Invalid Git response");
            encoded.push_str(line.trim());
        }
    }

    bail!("Git output was clipped. Select an individual file or narrow the history.")
}

pub fn parse_history(mut parts: BTreeMap<String, String>, offset: u32) -> Result<History> {
    let mut take = |name: &str| {
        parts
            .remove(name)
            .with_context(|| format!("Missing Git {name}"))
    };
    let root = take("root")?;
    let refs = take("refs")?;
    let trees = take("worktrees")?;
    let log = take("commits")?;
    let mut commits = Vec::new();
    let mut fields = log.split('\0');

    while let Some(id) = fields.next() {
        let id = id.trim_start_matches('\n');

        if id.is_empty() {
            break;
        }

        let parents = fields.next().context("Missing commit parents")?;
        let author = fields.next().context("Missing commit author")?;
        let seconds: u64 = fields.next().context("Missing commit date")?.parse()?;
        let subject = fields.next().context("Missing commit subject")?;
        commits.push(Commit {
            id: CommitId::from_str(id)?,
            parents: parents
                .split_whitespace()
                .map(CommitId::from_str)
                .collect::<Result<_>>()?,
            author: author.to_owned(),
            committed_at: seconds.checked_mul(1000).context("Commit date overflow")?,
            subject: subject.to_owned(),
        });
    }

    let next = if commits.len() > PAGE_SIZE {
        Some(
            offset
                .checked_add(u32::try_from(PAGE_SIZE)?)
                .context("History offset overflow")?,
        )
    } else {
        None
    };
    commits.truncate(PAGE_SIZE);
    let mut found_refs = Vec::new();
    let mut fields = refs.split('\0');

    while let Some(name) = fields.next() {
        let name = name.trim_start_matches('\n');

        if name.is_empty() {
            break;
        }

        let head = fields.next().context("Missing ref head")?;
        let symbolic = fields.next().context("Missing symbolic ref")?;

        if !symbolic.is_empty() {
            continue;
        }

        let (name, kind) = if let Some(n) = name.strip_prefix("refs/heads/") {
            (n, RefKind::Local)
        } else if let Some(n) = name.strip_prefix("refs/remotes/") {
            (n, RefKind::Remote)
        } else if let Some(n) = name.strip_prefix("refs/tags/") {
            (n, RefKind::Tag)
        } else {
            continue;
        };
        found_refs.push(GitRef {
            name: name.to_owned(),
            head: CommitId::from_str(head)?,
            kind,
        });
    }

    let mut worktrees = Vec::new();

    for record in trees.split("\0\0") {
        let mut path = None;
        let mut head = None;
        let mut branch = None;

        for field in record.split('\0') {
            if let Some(value) = field.strip_prefix("worktree ") {
                path = Some(value.to_owned());
            }

            if let Some(value) = field.strip_prefix("HEAD ")
                && !value.bytes().all(|b| b == b'0')
            {
                head = Some(CommitId::from_str(value)?);
            }

            if let Some(value) = field.strip_prefix("branch refs/heads/") {
                branch = Some(value.to_owned());
            }
        }

        if let Some(path) = path {
            worktrees.push(Worktree {
                path,
                head,
                branch,
                main: worktrees.is_empty(),
            });
        }
    }

    Ok(History {
        root: root.strip_suffix('\n').unwrap_or(&root).to_owned(),
        refs: found_refs,
        worktrees,
        commits,
        next,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangedFile {
    pub path: String,
    pub old_path: Option<String>,
    pub status: &'static str,
}

pub fn files_script(folder: &str, commit: &CommitId) -> String {
    framed(
        folder,
        &[(
            "files",
            format!(
                "git -c core.quotepath=off show --format= --root --diff-merges=first-parent --no-ext-diff --no-textconv --name-status --find-renames -z {} --",
                commit.as_ref()
            ),
        )],
    )
}

pub fn parse_files(output: &str) -> Result<Vec<ChangedFile>> {
    let mut fields = output.split('\0').filter(|s| !s.is_empty());
    let mut files = Vec::new();

    while let Some(status) = fields.next() {
        let path = fields.next().context("Missing changed file path")?;
        let (path, old_path, status) = match status.trim_start_matches('\n').as_bytes().first() {
            Some(b'R') => (
                fields.next().context("Missing renamed path")?,
                Some(path.to_owned()),
                "renamed",
            ),
            Some(b'C') => (fields.next().context("Missing copied path")?, None, "added"),
            Some(b'A') => (path, None, "added"),
            Some(b'D') => (path, None, "deleted"),
            Some(b'M' | b'T') => (path, None, "modified"),
            _ => bail!("Unknown Git file status"),
        };
        files.push(ChangedFile {
            path: path.to_owned(),
            old_path,
            status,
        });
    }

    Ok(files)
}

#[cfg(test)]
#[path = "tests/history.rs"]
mod tests;
