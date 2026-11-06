//! Files of the Artifacts tab's kinds a turn's commands made (a page a
//! script wrote, a PDF a build made), found when the turn ends by walking
//! the session's folder for ones written since the turn began. The turn
//! names them in a note of `(made <path>)` lines, which the apps read.

use crate::files::sync::Rules;
use std::{
    cmp::Reverse,
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

/// The kinds the Artifacts tab shows, by file extension.
const KINDS: &[&str] = &[
    "html", "htm", "md", "markdown", "svg", "png", "jpg", "jpeg", "gif", "webp", "pdf",
];
/// The most files named for one turn, the newest.
const MAX_PER_TURN: usize = 20;
/// How deep the walk goes, and how many entries it looks at, so a huge
/// folder cannot hold it up.
const MAX_DEPTH: usize = 8;
const MAX_SEEN: usize = 20_000;

/// The note that names `paths`, one `(made <path>)` line each.
pub(crate) fn note(paths: &[String]) -> String {
    paths
        .iter()
        .map(|path| format!("(made {path})"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The files under `cwd` of the Artifacts kinds last written at or after
/// `since`, newest first, relative to `cwd` with `/` between folders. Left
/// out: `written` (the files the turn's edits named, absolute or relative
/// to `cwd`), `.git`, links, and whatever project sync leaves out. Blocks.
pub(crate) fn find(cwd: &Path, since: SystemTime, written: &[PathBuf]) -> Vec<String> {
    let rules = match Rules::load(cwd) {
        Ok(rules) => rules,
        Err(e) => {
            log::debug!("no made files looked for in {}: {e:#}", cwd.display());
            return Vec::new();
        }
    };
    let written: Vec<PathBuf> = written.iter().map(|path| cwd.join(path)).collect();
    let mut found: Vec<(SystemTime, PathBuf)> = Vec::new();
    let mut folders = vec![(PathBuf::new(), 0usize)];
    let mut seen = 0usize;
    while let Some((folder, depth)) = folders.pop() {
        let Ok(entries) = fs::read_dir(cwd.join(&folder)) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > MAX_SEEN {
                break;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name();
            if name == ".git" || kind.is_symlink() {
                continue;
            }
            let rel = folder.join(&name);
            if rules.leaves_out(&rel, kind.is_dir()) {
                continue;
            }
            if kind.is_dir() {
                if depth < MAX_DEPTH {
                    folders.push((rel, depth + 1));
                }
                continue;
            }
            if !is_artifact(&rel) || written.contains(&cwd.join(&rel)) {
                continue;
            }
            let modified = entry.metadata().and_then(|meta| meta.modified());
            if let Ok(modified) = modified
                && modified >= since
            {
                found.push((modified, rel));
            }
        }
    }
    found.sort_by_key(|(modified, _)| Reverse(*modified));
    found
        .into_iter()
        .take(MAX_PER_TURN)
        .map(|(_, rel)| rel.to_string_lossy().replace('\\', "/"))
        .collect()
}

fn is_artifact(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| KINDS.iter().any(|kind| kind.eq_ignore_ascii_case(ext)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    fn only_new_artifacts_outside_ignored_folders_are_found() {
        let dir = TempDir::new().expect("a temp folder");
        let cwd = dir.path();
        fs::write(cwd.join("Cargo.toml"), "").expect("write");
        fs::write(cwd.join("old.html"), "<p>old</p>").expect("write");
        let since = SystemTime::now() + Duration::from_secs(60);
        // Files stamped after `since`, so the test needs no sleep.
        let stamp = |path: &Path| {
            let file = fs::File::options().write(true).open(path).expect("open");
            file.set_modified(since + Duration::from_secs(1))
                .expect("stamp");
        };
        for path in [
            "out/coverage.pdf",
            "target/doc/index.html",
            "notes.txt",
            "written.md",
        ] {
            let path = cwd.join(path);
            fs::create_dir_all(path.parent().expect("a folder")).expect("mkdir");
            fs::write(&path, "x").expect("write");
            stamp(&path);
        }
        let found = find(cwd, since, &[PathBuf::from("written.md")]);
        assert_eq!(found, ["out/coverage.pdf"]);
        assert_eq!(note(&found), "(made out/coverage.pdf)");
    }
}
