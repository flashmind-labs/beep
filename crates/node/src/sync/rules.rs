//! Which paths of a project sync, in the order the rules apply:
//!
//! 1. `.git` never does; in a git repository only files git ignores do,
//!    since git carries the rest between machines.
//! 2. The project's `.gitignore`, in a folder that is not a git repository.
//! 3. Defaults for the kinds of project its marker files show, such as
//!    `target/` beside `Cargo.toml`. A plain folder gets them only when it
//!    has no `.gitignore`; a git repository always does, since what git
//!    ignores is mostly build output.
//! 4. `.beepignore`, in `.gitignore` syntax, adds rules, and a `!` rule
//!    keeps a path the rules above leave out.
//!
//! Everything here blocks.

use anyhow::{Context, Result, bail};
use beep_proto::node::sync::Rule;
use ignore::{
    Match,
    gitignore::{Gitignore, GitignoreBuilder},
};
use std::{
    collections::HashSet,
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

/// The project's own ignore file.
pub(super) const BEEPIGNORE: &str = ".beepignore";
const GITIGNORE: &str = ".gitignore";

/// What a kind of project leaves out, found by a marker file.
struct Defaults {
    /// What the marker file is called, or ends with for `*.ext` markers.
    markers: &'static [&'static str],
    rules: &'static [&'static str],
}

/// The defaults by marker, as the issue lists them, then Gradle, Xcode and
/// Godot.
const DEFAULTS: &[Defaults] = &[
    Defaults {
        markers: &["Cargo.toml"],
        rules: &["target/"],
    },
    Defaults {
        markers: &["package.json"],
        rules: &["node_modules/", ".next/", "dist/", ".turbo/"],
    },
    Defaults {
        markers: &["pyproject.toml", "requirements.txt"],
        rules: &[".venv/", "__pycache__/", ".pytest_cache/", "*.pyc"],
    },
    Defaults {
        markers: &["*.uproject"],
        rules: &[
            "Binaries/",
            "Intermediate/",
            "Saved/",
            "DerivedDataCache/",
            ".vs/",
        ],
    },
    Defaults {
        markers: &["*.blend"],
        rules: &["*.blend1", "*.blend2"],
    },
    Defaults {
        markers: &[
            "build.gradle",
            "build.gradle.kts",
            "settings.gradle",
            "settings.gradle.kts",
        ],
        rules: &[
            ".gradle/",
            "build/",
            ".kotlin/",
            ".cxx/",
            ".externalNativeBuild/",
            "local.properties",
        ],
    },
    Defaults {
        markers: &["*.xcodeproj", "*.xcworkspace", "Package.swift"],
        rules: &["DerivedData/", ".build/", "xcuserdata/", "*.xcuserstate"],
    },
    Defaults {
        markers: &["project.godot"],
        rules: &[".godot/", ".import/"],
    },
];

fn is_marker(name: &str, marker: &str) -> bool {
    match marker.strip_prefix('*') {
        Some(ext) => name.len() > ext.len() && name.ends_with(ext),
        None => name == marker,
    }
}

/// Why a path is left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Skip {
    /// One of the rules the user can see.
    Rule {
        rule: Rule,
        /// The marker file of a default.
        marker: Option<String>,
    },
    /// Sync's own partial files, never listed.
    Own,
}

impl Skip {
    fn rule(rule: Rule) -> Self {
        Self::Rule { rule, marker: None }
    }
}

/// One marker's rules.
#[derive(Debug)]
struct Found {
    marker: String,
    matcher: Gitignore,
}

/// A project's rules, read from its folder.
#[derive(Debug)]
pub(super) struct Rules {
    /// The folder is a git repository.
    git: bool,
    gitignore: Option<Gitignore>,
    defaults: Vec<Found>,
    beepignore: Option<Gitignore>,
}

/// Whether `name` is a partial folder or file sync or a transfer writes.
pub(super) fn is_own(name: &str) -> bool {
    name.starts_with(".beep-") && (name.ends_with(".partial") || name.ends_with(".tmp"))
}

impl Rules {
    /// Reads the rules of the project in `root`: its `.gitignore` and
    /// `.beepignore`, and the marker files in it and its first-level
    /// folders.
    pub(super) fn load(root: &Path) -> Result<Self> {
        let git = root.join(".git").exists();
        let gitignore = if git {
            None
        } else {
            read_ignore(root, GITIGNORE)?
        };
        let defaults = if gitignore.is_some() {
            Vec::new()
        } else {
            defaults(root)?
        };
        Ok(Self {
            git,
            gitignore,
            defaults,
            beepignore: read_ignore(root, BEEPIGNORE)?,
        })
    }

    /// Whether the folder is a git repository.
    pub(super) fn is_git(&self) -> bool {
        self.git
    }

    /// Why `path`, relative to the project folder, is left out, or `None`
    /// when it syncs as far as these rules go. In a git repository a kept
    /// path must also be one git ignores (`GitIgnored`).
    pub(super) fn skip(&self, path: &Path, is_dir: bool) -> Option<Skip> {
        let mut parts = path.components().peekable();
        while let Some(part) = parts.next() {
            let Component::Normal(name) = part else {
                return Some(Skip::Own);
            };
            if name == ".git" {
                return Some(Skip::rule(Rule::Git));
            }
            if parts.peek().is_none() && name.to_str().is_some_and(is_own) {
                return Some(Skip::Own);
            }
        }
        if let Some(beepignore) = &self.beepignore {
            match beepignore.matched(path, is_dir) {
                Match::Ignore(_) => return Some(Skip::rule(Rule::BeepIgnore)),
                Match::Whitelist(_) => return None,
                Match::None => {}
            }
        }
        if let Some(gitignore) = &self.gitignore
            && gitignore.matched(path, is_dir).is_ignore()
        {
            return Some(Skip::rule(Rule::GitIgnore));
        }
        self.defaults
            .iter()
            .find(|found| found.matcher.matched(path, is_dir).is_ignore())
            .map(|found| Skip::Rule {
                rule: Rule::Default,
                marker: Some(found.marker.clone()),
            })
    }

    /// Whether `path` or a folder it lies in is left out; `is_dir` says
    /// whether `path` itself is a folder.
    pub(super) fn skips_any(&self, path: &Path, is_dir: bool) -> bool {
        let parts: Vec<_> = path.components().collect();
        let mut prefix = PathBuf::new();
        for (at, part) in parts.iter().enumerate() {
            prefix.push(part);
            let last = at + 1 == parts.len();
            if self.skip(&prefix, !last || is_dir).is_some() {
                return true;
            }
        }
        false
    }

    /// The marker files whose defaults apply.
    #[cfg(test)]
    fn markers(&self) -> Vec<&str> {
        self.defaults.iter().map(|f| f.marker.as_str()).collect()
    }
}

fn read_ignore(root: &Path, name: &str) -> Result<Option<Gitignore>> {
    let path = root.join(name);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => return Ok(None),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    }
    let mut builder = GitignoreBuilder::new(root);
    if let Some(e) = builder.add(&path) {
        log::warn!("some rules in {} were skipped: {e}", path.display());
    }
    let built = builder
        .build()
        .with_context(|| format!("reading {}", path.display()))?;
    Ok(Some(built))
}

/// The defaults for every marker in `root` or one of its first-level
/// folders, each marker kind once.
fn defaults(root: &Path) -> Result<Vec<Found>> {
    let mut names = Vec::new();
    let collect = |dir: &Path, names: &mut Vec<String>| -> Result<Vec<String>> {
        let mut folders = Vec::new();
        let listing = match fs::read_dir(dir) {
            Ok(listing) => listing,
            // A first-level folder that went away or cannot be read.
            Err(_) if dir != root => return Ok(folders),
            Err(e) => return Err(e).with_context(|| format!("listing {}", dir.display())),
        };
        for item in listing.flatten() {
            let Some(name) = item.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if item.file_type().is_ok_and(|kind| kind.is_dir()) && !name.starts_with('.') {
                folders.push(name.clone());
            }
            names.push(name);
        }
        Ok(folders)
    };
    for folder in collect(root, &mut names)? {
        collect(&root.join(folder), &mut names)?;
    }
    let mut found = Vec::new();
    for set in DEFAULTS {
        let Some(marker) = set
            .markers
            .iter()
            .find_map(|marker| names.iter().find(|name| is_marker(name, marker)))
        else {
            continue;
        };
        let mut builder = GitignoreBuilder::new(root);
        for rule in set.rules {
            builder
                .add_line(None, rule)
                .with_context(|| format!("the default rule {rule}"))?;
        }
        found.push(Found {
            marker: marker.clone(),
            matcher: builder.build()?,
        });
    }
    Ok(found)
}

/// What git ignores in a repository: files, and folders it ignores whole,
/// by path relative to the repository, folders without their `/`.
#[derive(Debug, Default)]
pub(super) struct GitIgnored {
    files: HashSet<String>,
    folders: HashSet<String>,
}

impl GitIgnored {
    /// Asks git in `root`.
    pub(super) fn read(root: &Path) -> Result<Self> {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args([
                "ls-files",
                "-z",
                "--others",
                "--ignored",
                "--exclude-standard",
                "--directory",
            ])
            .output()
            .context("running git, which a git repository needs to sync")?;
        if !output.status.success() {
            bail!(
                "git could not list the ignored files: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let mut ignored = Self::default();
        for path in output.stdout.split(|&b| b == 0).filter(|p| !p.is_empty()) {
            let Ok(path) = std::str::from_utf8(path) else {
                continue;
            };
            match path.strip_suffix('/') {
                Some(folder) => ignored.folders.insert(folder.to_owned()),
                None => ignored.files.insert(path.to_owned()),
            };
        }
        Ok(ignored)
    }

    /// Whether git ignores `path`, a file, or a folder it lies in.
    pub(super) fn contains(&self, path: &str) -> bool {
        if self.files.contains(path) || self.folders.contains(path) {
            return true;
        }
        path.match_indices('/')
            .any(|(at, _)| self.folders.contains(&path[..at]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn project(files: &[&str]) -> TempDir {
        let dir = TempDir::new().expect("a temp folder");
        for file in files {
            let path = dir.path().join(file);
            if let Some(stripped) = file.strip_suffix('/') {
                fs::create_dir_all(dir.path().join(stripped)).unwrap();
                continue;
            }
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }
        dir
    }

    fn skipped(rules: &Rules, path: &str, is_dir: bool) -> Option<Rule> {
        match rules.skip(Path::new(path), is_dir) {
            Some(Skip::Rule { rule, .. }) => Some(rule),
            Some(Skip::Own) => Some(Rule::Other),
            None => None,
        }
    }

    /// A marker file, the paths its defaults leave out (and whether each
    /// is a folder), and paths they keep.
    struct Case {
        marker: &'static str,
        left_out: &'static [(&'static str, bool)],
        kept: &'static [&'static str],
    }

    fn case(
        marker: &'static str,
        left_out: &'static [(&'static str, bool)],
        kept: &'static [&'static str],
    ) -> Case {
        Case {
            marker,
            left_out,
            kept,
        }
    }

    #[test]
    fn every_marker_brings_its_defaults() {
        let cases = [
            case("Cargo.toml", &[("target", true)], &["src/main.rs"]),
            case(
                "package.json",
                &[
                    ("node_modules", true),
                    (".next", true),
                    ("dist", true),
                    (".turbo", true),
                    ("web/node_modules", true),
                ],
                &["index.js"],
            ),
            case(
                "pyproject.toml",
                &[
                    (".venv", true),
                    ("__pycache__", true),
                    (".pytest_cache", true),
                    ("pkg/mod.pyc", false),
                ],
                &["pkg/mod.py"],
            ),
            case(
                "requirements.txt",
                &[(".venv", true), ("a.pyc", false)],
                &["a.py"],
            ),
            case(
                "Game.uproject",
                &[
                    ("Binaries", true),
                    ("Intermediate", true),
                    ("Saved", true),
                    ("DerivedDataCache", true),
                    (".vs", true),
                ],
                &["Content/Map.umap"],
            ),
            case(
                "scene.blend",
                &[("scene.blend1", false), ("scene.blend2", false)],
                &["scene.blend"],
            ),
            case(
                "build.gradle.kts",
                &[
                    (".gradle", true),
                    ("app/build", true),
                    ("local.properties", false),
                ],
                &["app/src/Main.kt"],
            ),
            case(
                "App.xcodeproj/",
                &[
                    ("DerivedData", true),
                    (".build", true),
                    ("App.xcodeproj/xcuserdata", true),
                    ("UserInterfaceState.xcuserstate", false),
                ],
                &["App/ContentView.swift"],
            ),
            case(
                "Package.swift",
                &[(".build", true)],
                &["Sources/main.swift"],
            ),
            case(
                "project.godot",
                &[(".godot", true), (".import", true)],
                &["scenes/main.tscn"],
            ),
        ];
        for Case {
            marker,
            left_out,
            kept,
        } in cases
        {
            let dir = project(&[marker]);
            let rules = Rules::load(dir.path()).unwrap();
            assert_eq!(rules.markers(), [marker.trim_end_matches('/')], "{marker}");
            for (path, is_dir) in left_out {
                assert_eq!(
                    skipped(&rules, path, *is_dir),
                    Some(Rule::Default),
                    "{marker}: {path}"
                );
            }
            for path in kept {
                assert_eq!(skipped(&rules, path, false), None, "{marker}: {path}");
            }
        }
    }

    #[test]
    fn a_marker_one_folder_down_counts_and_markers_combine() {
        let dir = project(&["app/package.json", "Cargo.toml", "deep/er/pyproject.toml"]);
        let rules = Rules::load(dir.path()).unwrap();
        assert_eq!(rules.markers(), ["Cargo.toml", "package.json"]);
        assert_eq!(
            skipped(&rules, "app/node_modules", true),
            Some(Rule::Default)
        );
        assert_eq!(skipped(&rules, "target", true), Some(Rule::Default));
        // Two folders down is too deep for a marker.
        assert_eq!(skipped(&rules, ".venv", true), None);
        let Some(Skip::Rule { marker, .. }) = rules.skip(Path::new("target"), true) else {
            panic!("target is left out");
        };
        assert_eq!(marker.as_deref(), Some("Cargo.toml"));
    }

    #[test]
    fn git_never_syncs_and_own_partials_are_hidden() {
        let dir = project(&["notes.txt"]);
        let rules = Rules::load(dir.path()).unwrap();
        assert_eq!(skipped(&rules, ".git", true), Some(Rule::Git));
        assert_eq!(skipped(&rules, "sub/.git", false), Some(Rule::Git));
        assert_eq!(skipped(&rules, "sub/.git/config", false), Some(Rule::Git));
        assert_eq!(
            rules.skip(Path::new(".beep-1-2.partial"), true),
            Some(Skip::Own)
        );
        assert_eq!(skipped(&rules, "notes.txt", false), None);
    }

    #[test]
    fn a_path_inside_a_skipped_folder_is_skipped() {
        let dir = project(&["Cargo.toml"]);
        let rules = Rules::load(dir.path()).unwrap();
        assert!(rules.skips_any(Path::new("target/debug/app"), false));
        assert!(rules.skips_any(Path::new("crates/x/target"), true));
        assert!(rules.skips_any(Path::new(".git/index"), false));
        assert!(!rules.skips_any(Path::new("src/main.rs"), false));
    }

    #[test]
    fn a_gitignore_replaces_the_defaults_outside_git() {
        let dir = project(&["Cargo.toml"]);
        fs::write(dir.path().join(".gitignore"), "*.log\nout/\n").unwrap();
        let rules = Rules::load(dir.path()).unwrap();
        assert!(!rules.is_git());
        assert!(rules.markers().is_empty());
        assert_eq!(skipped(&rules, "run.log", false), Some(Rule::GitIgnore));
        assert_eq!(skipped(&rules, "out", true), Some(Rule::GitIgnore));
        assert_eq!(skipped(&rules, "target", true), None);
    }

    #[test]
    fn beepignore_adds_rules_and_keeps_what_others_leave_out() {
        let dir = project(&["Cargo.toml", "package.json"]);
        fs::write(dir.path().join(".beepignore"), "renders/\n!dist/\n*.tmp\n").unwrap();
        let rules = Rules::load(dir.path()).unwrap();
        assert_eq!(skipped(&rules, "renders", true), Some(Rule::BeepIgnore));
        assert_eq!(skipped(&rules, "a.tmp", false), Some(Rule::BeepIgnore));
        // `!dist/` keeps the folder the package.json defaults leave out.
        assert_eq!(skipped(&rules, "dist", true), None);
        assert_eq!(skipped(&rules, "target", true), Some(Rule::Default));

        fs::write(dir.path().join(".gitignore"), "*.tmp\nnotes.txt\n").unwrap();
        fs::write(dir.path().join(".beepignore"), "!notes.txt\n").unwrap();
        let rules = Rules::load(dir.path()).unwrap();
        assert_eq!(skipped(&rules, "notes.txt", false), None);
        assert_eq!(skipped(&rules, "a.tmp", false), Some(Rule::GitIgnore));
    }

    #[test]
    fn a_git_repository_keeps_the_defaults_and_asks_git() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let dir = project(&[
            "Cargo.toml",
            "src/main.rs",
            "target/debug/app",
            ".env",
            "data/big.bin",
        ]);
        fs::write(dir.path().join(".gitignore"), "target/\n.env\ndata/\n").unwrap();
        let git = |args: &[&str]| {
            let status = Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .output()
                .unwrap();
            assert!(status.status.success(), "{status:?}");
        };
        git(&["init", "-q"]);
        git(&["add", "Cargo.toml", "src/main.rs", ".gitignore"]);
        let rules = Rules::load(dir.path()).unwrap();
        assert!(rules.is_git());
        // The defaults still leave out build output git ignores.
        assert_eq!(skipped(&rules, "target", true), Some(Rule::Default));
        let ignored = GitIgnored::read(dir.path()).unwrap();
        assert!(ignored.contains(".env"));
        assert!(ignored.contains("data/big.bin"));
        assert!(ignored.contains("target/debug/app"));
        assert!(!ignored.contains("src/main.rs"));
        assert!(!ignored.contains("Cargo.toml"));
    }
}
