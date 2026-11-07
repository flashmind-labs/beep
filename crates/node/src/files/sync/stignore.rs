//! Syncthing's ignore files. A project under a Syncthing folder (a whole share,
//! usually a folder above it) keeps honoring them once Syncthing is paused, or
//! Beep's sync would send what the user told Syncthing to leave out. Everything
//! here blocks.

use crate::files::syncthing::{PROJECT_IGNORE_BEGIN, PROJECT_IGNORE_END};
use anyhow::{Context, Result, bail};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::{
    collections::HashSet,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

/// Whether Syncthing excludes the project's directory, including an
/// exclusion for every child of a project that is itself a shared folder.
pub(crate) fn project_ignored(project: &Path, share: &Path) -> Result<bool> {
    let file = share.join(STIGNORE);
    if !file.exists() {
        return Ok(false);
    }
    let mut lines = Vec::new();
    read_lines_for_project(&file, &mut HashSet::new(), &mut lines, None)?;
    let relative = project.strip_prefix(share)?;
    // A preceding exception may include a child even when its directory
    // matches the blanket rule. Keep ownership with Syncthing in that case.
    let mut exception = false;
    for line in lines {
        if line.starts_with('!') {
            exception = true;
            continue;
        }
        let mut builder = GitignoreBuilder::new(share);
        builder.add_line(None, &line)?;
        let matcher = builder.build()?;
        let whole_share =
            relative.as_os_str().is_empty() && matches!(line.as_str(), "/**" | "**" | "*");
        if whole_share
            || matcher
                .matched_path_or_any_parents(relative, true)
                .is_ignore()
        {
            return Ok(!exception);
        }
    }
    Ok(false)
}

pub(super) const STIGNORE: &str = ".stignore";
/// How deep `#include`s may nest.
const MAX_INCLUDES: usize = 16;

/// One folder's `.stignore`, for one project at or below that folder.
#[derive(Debug)]
pub(super) struct Stignore {
    /// The project's path from the ignore file's folder.
    prefix: PathBuf,
    matcher: Gitignore,
}

impl Stignore {
    /// The ignore files in `root` and the folders above it. The folder is not
    /// asked whether Syncthing still keeps it: rules the user wrote hold
    /// whether Syncthing runs, paused its folder or has gone. Fails on one it
    /// cannot read in full, saying why.
    pub(super) fn load_above(root: &Path) -> Result<Vec<Self>> {
        let mut found = Vec::new();
        for folder in root.ancestors() {
            let file = folder.join(STIGNORE);
            if !fs::symlink_metadata(&file).is_ok_and(|meta| meta.is_file()) {
                continue;
            }
            let matcher = parse_for_project(&file, folder, Some(root)).with_context(|| {
                format!(
                    "{} has rules Beep cannot follow, so it does not take over from Syncthing",
                    file.display()
                )
            })?;
            found.push(Self {
                prefix: root.strip_prefix(folder)?.to_owned(),
                matcher,
            });
        }
        Ok(found)
    }

    /// Whether the file leaves out `path`, relative to the project.
    pub(super) fn ignores(&self, path: &Path, is_dir: bool) -> bool {
        self.matcher
            .matched_path_or_any_parents(self.prefix.join(path), is_dir)
            .is_ignore()
    }
}

fn parse_for_project(file: &Path, folder: &Path, project: Option<&Path>) -> Result<Gitignore> {
    let mut lines = Vec::new();
    let mut open = HashSet::new();
    read_lines_for_project(file, &mut open, &mut lines, project)?;
    let mut builder = GitignoreBuilder::new(folder);
    // Syncthing's first match wins, and gitignore's last.
    for line in lines.iter().rev() {
        builder
            .add_line(None, line)
            .with_context(|| format!("the pattern {line}"))?;
    }
    Ok(builder.build()?)
}

fn read_lines_for_project(
    file: &Path,
    open: &mut HashSet<PathBuf>,
    out: &mut Vec<String>,
    project: Option<&Path>,
) -> Result<()> {
    if open.len() >= MAX_INCLUDES || !open.insert(file.to_owned()) {
        bail!("#include of {} goes around or too deep", file.display());
    }
    let text = match fs::read_to_string(file) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => bail!("{} is missing", file.display()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", file.display())),
    };
    let lines: Vec<_> = text.trim_start_matches('\u{feff}').lines().collect();
    for (at, line) in lines.iter().enumerate() {
        // Only Beep's own whole-project exclusion is omitted. User rules,
        // including identical patterns, still apply to Beep's file list.
        if project.is_some_and(|project| {
            let Some(parent) = file.parent() else {
                return false;
            };
            let Ok(relative) = project.strip_prefix(parent) else {
                return false;
            };
            let relative = relative.to_string_lossy().replace('\\', "/");
            let pattern = if relative.is_empty() {
                "/**".to_owned()
            } else {
                format!("/{relative}")
            };
            *line == pattern
                && at > 0
                && lines[at - 1] == PROJECT_IGNORE_BEGIN
                && lines.get(at + 1) == Some(&PROJECT_IGNORE_END)
        }) {
            continue;
        }
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if let Some(name) = line.strip_prefix("#include ") {
            let parent = file.parent().unwrap_or(Path::new(""));
            read_lines_for_project(&parent.join(name.trim()), open, out, project)?;
        } else if line.starts_with("#escape") {
            bail!("the directive {line}");
        } else if line.starts_with('#') {
            // Syncthing reads it as a pattern for a name starting with `#`, which in
            // practice is a comment people write; gitignore reads it as one.
            continue;
        } else {
            out.push(translate(line)?);
        }
    }
    open.remove(file);
    Ok(())
}

/// One Syncthing pattern as a gitignore line. Both skip what lies inside a
/// match. `(?d)` asks to delete ignored files that stop a folder's removal,
/// which this sync never does for files it did not sync, so it changes nothing.
fn translate(line: &str) -> Result<String> {
    let (mut rest, mut negated, mut folded) = (line, false, false);
    loop {
        if let Some(after) = rest.strip_prefix("(?d)") {
            rest = after;
        } else if let Some(after) = rest.strip_prefix("(?i)") {
            folded = true;
            rest = after;
        } else if let Some(after) = rest.strip_prefix('!') {
            negated = true;
            rest = after;
        } else if rest.starts_with("(?") {
            bail!("the prefix in {line}");
        } else {
            break;
        }
    }
    let body = rest.trim_end_matches('/');
    if body.is_empty() {
        bail!("the empty pattern {line}");
    }
    // Gitignore would read these as literal text, and match nothing.
    let count = |c: char| body.matches(c).count();
    if count('[') != count(']') || count('{') != count('}') {
        bail!("the unbalanced pattern {line}");
    }
    let body = if folded { fold(body) } else { body.to_owned() };
    // Without a leading `/`, Syncthing matches at any depth, even with a `/`
    // inside, where gitignore would tie it to the root.
    let anchored = if !body.starts_with('/') && !body.starts_with("**/") && body.contains('/') {
        format!("**/{body}")
    } else {
        body
    };
    let bang = if negated { "!" } else { "" };
    // A body starting with `#` would read as a comment.
    let escape = if anchored.starts_with('#') { "\\" } else { "" };
    Ok(format!("{bang}{escape}{anchored}"))
}

/// `pattern` matching either case of each letter outside a `[...]`.
fn fold(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len() * 2);
    let mut chars = pattern.chars();
    let mut in_class = false;
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                out.push(c);
                out.extend(chars.next());
            }
            '[' => {
                in_class = true;
                out.push(c);
            }
            ']' => {
                in_class = false;
                out.push(c);
            }
            c if !in_class && c.is_alphabetic() => {
                out.push('[');
                out.extend(c.to_lowercase());
                out.extend(c.to_uppercase());
                out.push(']');
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// The rules of `text`, for a project in `share/project`.
    fn rules(text: &str) -> (TempDir, Vec<Stignore>) {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("share/project");
        fs::create_dir_all(&root).unwrap();
        fs::write(dir.path().join("share").join(STIGNORE), text).unwrap();
        let found = Stignore::load_above(&root).unwrap();
        (dir, found)
    }

    fn ignored(rules: &[Stignore], path: &str) -> bool {
        rules.iter().any(|st| st.ignores(Path::new(path), false))
    }

    #[test]
    fn a_rule_at_the_shares_root_reaches_a_project_below_it() {
        let (_dir, rules) = rules(
            "// a comment\n\
             (?d)/project/data/captures\n\
             /elsewhere\n\
             (?d)**/cache/*.bin\n\
             sub/dir\n\
             *.raw\n",
        );
        assert!(ignored(&rules, "data/captures"));
        assert!(ignored(&rules, "data/captures/2026/a.mov"));
        assert!(!ignored(&rules, "data/other.mov"));
        assert!(!ignored(&rules, "elsewhere"));
        assert!(ignored(&rules, "a/cache/x.bin"));
        // Without a leading `/`, at any depth, even with a `/` inside.
        assert!(ignored(&rules, "sub/dir/file"));
        assert!(ignored(&rules, "a/b/sub/dir/file"));
        assert!(ignored(&rules, "deep/take.raw"));
    }

    #[test]
    fn the_first_match_wins_and_case_folds_per_line() {
        let (_dir, rules) = rules("!keep.log\n*.log\n(?i)Secret\n(?d)(?i)!Plain.txt\nplain*\n");
        assert!(!ignored(&rules, "keep.log"));
        assert!(ignored(&rules, "other.log"));
        assert!(ignored(&rules, "SECRET"));
        assert!(ignored(&rules, "secret/x"));
        assert!(!ignored(&rules, "plain.TXT"));
        assert!(ignored(&rules, "plain.other"));
    }

    #[test]
    fn a_line_starting_with_a_hash_is_a_comment() {
        let (_dir, rules) = rules("# build output\n*.raw\n#also a note\n");
        assert!(ignored(&rules, "a.raw"));
        assert!(!ignored(&rules, "build output"));
    }

    #[test]
    fn a_project_exclusion_hands_ownership_to_beep_and_keeps_user_rules() {
        let dir = TempDir::new().unwrap();
        let project = dir.path().join("owner/widget");
        let other = dir.path().join("owner/other");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&other).unwrap();
        fs::write(
            dir.path().join(STIGNORE),
            format!("{PROJECT_IGNORE_BEGIN}\n/owner/widget\n{PROJECT_IGNORE_END}\n*.tmp\n"),
        )
        .unwrap();
        assert!(project_ignored(&project, dir.path()).unwrap());
        assert!(!project_ignored(&other, dir.path()).unwrap());
        let rules = Stignore::load_above(&project).unwrap();
        assert!(!ignored(&rules, "src/main.rs"));
        assert!(ignored(&rules, "cache.tmp"));
        // A matching rule written by the user still belongs to the user.
        fs::write(
            dir.path().join(STIGNORE),
            format!("{PROJECT_IGNORE_BEGIN}\n/owner/widget\n{PROJECT_IGNORE_END}\n/owner/widget\n"),
        )
        .unwrap();
        assert!(ignored(
            &Stignore::load_above(&project).unwrap(),
            "src/main.rs"
        ));
    }

    #[test]
    fn exceptions_and_rules_outside_the_share_cannot_release_ownership() {
        let dir = TempDir::new().unwrap();
        let share = dir.path().join("share");
        let project = share.join("widget");
        fs::create_dir_all(&project).unwrap();
        fs::write(dir.path().join(STIGNORE), "/share\n").unwrap();
        assert!(!project_ignored(&project, &share).unwrap());
        fs::write(share.join(STIGNORE), "!/widget/keep.txt\n/widget\n").unwrap();
        assert!(!project_ignored(&project, &share).unwrap());
        fs::write(share.join(STIGNORE), "/**\n").unwrap();
        assert!(project_ignored(&share, &share).unwrap());
    }

    #[test]
    fn an_include_counts_where_it_stands() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("project");
        fs::create_dir_all(&root).unwrap();
        fs::write(dir.path().join("more.txt"), "!kept.raw\n").unwrap();
        fs::write(root.join(STIGNORE), "#include ../more.txt\n*.raw\n").unwrap();
        let rules = Stignore::load_above(&root).unwrap();
        assert!(!ignored(&rules, "kept.raw"));
        assert!(ignored(&rules, "other.raw"));
    }

    #[test]
    fn rules_it_cannot_follow_are_refused_and_say_why() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("project");
        fs::create_dir_all(&root).unwrap();
        for (text, says) in [
            ("(?x)odd\n", "(?x)odd"),
            ("#escape=|\n", "#escape"),
            ("#include missing.txt\n", "missing.txt"),
            ("#include .stignore\n", "goes around"),
            ("[broken\n", "[broken"),
        ] {
            fs::write(root.join(STIGNORE), text).unwrap();
            let error = format!("{:#}", Stignore::load_above(&root).unwrap_err());
            assert!(error.contains(says), "{text}: {error}");
            assert!(error.contains("does not take over"), "{error}");
        }
    }
}
