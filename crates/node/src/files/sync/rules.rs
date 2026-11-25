//! Which paths of a project sync: `Rules::load` reads a project's rules and
//! `Rules::skip` applies them. Everything here blocks.

use super::gitdir;
use crate::files::syncthing::stignore::Stignore;
use anyhow::{Context, Result};
use beep_proto::node::sync::Rule;
use ignore::{
    Match,
    gitignore::{Gitignore, GitignoreBuilder},
};
use std::{
    collections::HashSet,
    fs,
    path::{Component, Path, PathBuf},
};

/// The project's own ignore file.
pub(super) const BEEPIGNORE: &str = ".beepignore";

/// These events can change which existing files belong to sync.
pub(super) fn changes_rules(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };

    matches!(name, BEEPIGNORE | ".stignore" | ".git")
        || path.components().count() <= 2
            && DEFAULTS
                .iter()
                .any(|set| set.markers.iter().any(|marker| is_marker(name, marker)))
}

/// What a kind of project leaves out, found by a marker file.
struct Defaults {
    /// What the marker file is called, or ends with for `*.ext` markers.
    markers: &'static [&'static str],
    rules: &'static [&'static str],
}

/// The defaults by marker: build output, caches and dependency folders,
/// never env files, local config or user content.
const DEFAULTS: &[Defaults] = &[
    Defaults {
        markers: &["Cargo.toml"],
        rules: &["target/"],
    },
    Defaults {
        markers: &["package.json"],
        rules: &[
            "node_modules/",
            ".next/",
            "dist/",
            ".turbo/",
            ".nuxt/",
            ".svelte-kit/",
            ".output/",
            ".parcel-cache/",
            ".vite/",
            "coverage/",
            ".angular/",
        ],
    },
    Defaults {
        markers: &["pyproject.toml", "requirements.txt"],
        rules: &[
            ".venv/",
            "__pycache__/",
            ".pytest_cache/",
            "*.pyc",
            ".mypy_cache/",
            ".ruff_cache/",
            ".tox/",
            "*.egg-info/",
        ],
    },
    Defaults {
        markers: &["pyproject.toml"],
        rules: &["build/"],
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
    // `ProjectSettings/` sits one folder down. `UserSettings/` is the
    // user's preferences and keeps syncing.
    Defaults {
        markers: &["ProjectVersion.txt"],
        rules: &["Library/", "Temp/", "Obj/", "Logs/", "Build/", "Builds/"],
    },
    Defaults {
        markers: &["*.csproj", "*.fsproj", "*.sln"],
        rules: &["bin/", "obj/"],
    },
    Defaults {
        markers: &["pom.xml"],
        rules: &["target/"],
    },
    Defaults {
        markers: &["CMakeLists.txt"],
        rules: &["build/", "cmake-build-*/"],
    },
    Defaults {
        markers: &["mix.exs"],
        rules: &["_build/", "deps/"],
    },
    Defaults {
        markers: &["Gemfile"],
        rules: &["vendor/bundle/"],
    },
    Defaults {
        markers: &["composer.json"],
        rules: &["vendor/"],
    },
    Defaults {
        markers: &["pubspec.yaml"],
        rules: &[".dart_tool/", "build/"],
    },
    Defaults {
        markers: &["build.zig"],
        rules: &["zig-cache/", ".zig-cache/", "zig-out/"],
    },
    Defaults {
        markers: &["*.cabal", "stack.yaml"],
        rules: &["dist-newstyle/", ".stack-work/"],
    },
    Defaults {
        markers: &["*.tf"],
        rules: &[".terraform/"],
    },
    Defaults {
        markers: &["build.sbt"],
        rules: &["target/", "project/target/", ".bloop/", ".metals/"],
    },
];

/// Left out of every project, whatever its kind: build output and caches
/// nothing wants carried, and folders that belong to the machine that made
/// them (a linked worktree's `.git` file names that machine's paths).
const ALWAYS: &[&str] = &[
    "target",
    "node_modules",
    ".DS_Store",
    ".gradle",
    ".cxx",
    ".externalNativeBuild",
    ".build",
    "DerivedData",
    "__pycache__",
    ".venv",
    "venv",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".tox",
    ".next",
    ".nuxt",
    ".output",
    ".svelte-kit",
    ".turbo",
    ".astro",
    ".vite",
    ".open-next",
    ".cache",
    ".parcel-cache",
    ".nyc_output",
    "**/android/build",
    "**/android/**/build",
    "**/ios/build",
    "**/.worktrees",
    "**/.claude/worktrees",
    "*.sync-conflict-*",
    // Editor and OS temporary files.
    "*.swp",
    "*.swo",
    "*.swx",
    "*~",
    "4913",
    ".#*",
    "\\#*#",
    "*___jb_tmp___",
    "*___jb_old___",
    "Thumbs.db",
    "desktop.ini",
    "._*",
    "*.tmp",
    "*.crdownload",
    "*.part",
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
    /// Sync's own partial files and Syncthing's folders, never listed.
    Own,
}

impl Skip {
    fn rule(rule: Rule) -> Self {
        Self::Rule { rule, marker: None }
    }
}

/// One set of default rules.
#[derive(Debug)]
struct Found {
    /// The marker file that brought them; `None` for `ALWAYS`.
    marker: Option<String>,
    matcher: Gitignore,
}

/// A project's rules, read from its folder, applied in this order:
/// 1. The root's `.git` is collected for a new peer's first copy only,
///    even when Syncthing excludes it. A `.git` further
///    down never does, and a folder whose `.git` is a file (a linked worktree
///    or a submodule) stays on its machine (see `scan::walk`).
/// 2. The `.stignore` of every folder at or above the project: what the user
///    told Syncthing to leave out stays out once Syncthing pauses.
/// 3. `.beepignore` (`.gitignore` syntax), then the project's rules from the
///    mesh, add rules; a `!` rule keeps a path the rules below leave out.
/// 4. `ALWAYS`, and the defaults for the project kinds its marker files show
///    (`target/` beside `Cargo.toml`). A file git tracks stays in spite of
///    them, or `git status` would call it deleted elsewhere. A `.gitignore`
///    is not read, as it would also leave out `.env` and other files worth
///    keeping.
#[derive(Debug)]
pub(crate) struct Rules {
    /// What git tracks here, and the folders that hold it.
    tracked: HashSet<String>,
    /// Why the root's `.git` stays here: it is a file, or a rule says so.
    git_stays: Option<Skip>,
    stignores: Vec<Stignore>,
    defaults: Vec<Found>,
    beepignore: Option<Gitignore>,
    ignored_parent: Option<(PathBuf, Box<Rules>)>,
}

/// The file that tells a project folder from another (see `root`).
pub(super) const MARKER: &str = ".beep-sync";

/// Whether `name` is a partial folder or file sync or a transfer writes,
/// or sync's marker.
pub(super) fn is_own(name: &str) -> bool {
    name == MARKER
        || (name.starts_with(".beep-") && (name.ends_with(".partial") || name.ends_with(".tmp")))
}

/// Whether `name` is Syncthing's own: its folders, whose marker would tell a
/// peer that Syncthing keeps the project there, its temporary files, and
/// its ignore file, which each machine keeps for itself.
pub(super) fn is_syncthings(name: &str) -> bool {
    name == ".stfolder"
        || name == ".stversions"
        || name == ".stignore"
        || name.starts_with(".syncthing")
        || name.starts_with("~syncthing~")
}

impl Rules {
    /// Reads the rules of the project in `root`: the defaults for the
    /// marker files in it and its first-level folders, its `.beepignore`
    /// followed by `extra` (the project's rules from the mesh), and the
    /// `.stignore` files at or above it. Fails on a `.stignore` it cannot
    /// read, since syncing then could send what the user left out.
    pub(crate) fn load(root: &Path, extra: &str) -> Result<Self> {
        let mut rules = Self {
            tracked: tracked(root),
            ignored_parent: None,
            git_stays: None,
            stignores: Stignore::load_above(root)?,
            defaults: defaults(root)?,
            beepignore: read_ignore(root, BEEPIGNORE, extra)?,
        };
        let git = Path::new(gitdir::GIT);
        rules.git_stays = if fs::symlink_metadata(root.join(git)).is_ok_and(|meta| !meta.is_dir()) {
            Some(Skip::rule(Rule::Git))
        } else if rules.beepignored(git, true).is_ignore() {
            Some(Skip::rule(Rule::BeepIgnore))
        } else {
            None
        };
        Ok(rules)
    }

    pub(super) fn ignored_within(&mut self, relative: PathBuf, parent: Rules) {
        self.git_stays = Some(Skip::rule(Rule::Git));
        self.ignored_parent = Some((relative, Box::new(parent)));
    }

    pub(super) fn ignored_only(&self) -> bool {
        self.ignored_parent.is_some()
    }

    fn stignored(&self, path: &Path, is_dir: bool) -> bool {
        self.stignores.iter().any(|st| st.ignores(path, is_dir))
    }

    fn beepignored(&self, path: &Path, is_dir: bool) -> Match<()> {
        match &self.beepignore {
            Some(ignore) => ignore.matched_path_or_any_parents(path, is_dir).map(|_| ()),
            None => Match::None,
        }
    }

    /// Whether the project's `.git` syncs here.
    pub(super) fn syncs_git(&self) -> bool {
        self.git_stays.is_none()
    }

    /// Why `path`, relative to the project folder, is left out, or `None`
    /// when it syncs.
    pub(super) fn skip(&self, path: &Path, is_dir: bool) -> Option<Skip> {
        if let Some((prefix, parent)) = &self.ignored_parent
            && parent.skips_any(&prefix.join(path), is_dir)
        {
            return Some(Skip::rule(Rule::Other));
        }

        let mut parts = path.components().peekable();

        if parts
            .peek()
            .is_some_and(|part| part.as_os_str() == gitdir::GIT)
        {
            return self.skip_in_git(path, is_dir);
        }

        while let Some(part) = parts.next() {
            let Component::Normal(name) = part else {
                return Some(Skip::Own);
            };

            if name == gitdir::GIT {
                return Some(Skip::rule(Rule::Git));
            }

            if name.to_str().is_some_and(is_syncthings) {
                return Some(Skip::Own);
            }

            if parts.peek().is_none() && name.to_str().is_some_and(is_own) {
                return Some(Skip::Own);
            }
        }

        if self.stignored(path, is_dir) {
            return Some(Skip::rule(Rule::Other));
        }

        match self.beepignored(path, is_dir) {
            Match::Ignore(()) => return Some(Skip::rule(Rule::BeepIgnore)),
            Match::Whitelist(()) => return None,
            Match::None => {}
        }

        if !self.tracked.is_empty()
            && self
                .tracked
                .contains(&*path.to_string_lossy().replace('\\', "/"))
        {
            return None;
        }

        self.defaults
            .iter()
            .find(|found| {
                found
                    .matcher
                    .matched_path_or_any_parents(path, is_dir)
                    .is_ignore()
            })
            .map(|found| Skip::Rule {
                rule: Rule::Default,
                marker: found.marker.clone(),
            })
    }

    fn skip_in_git(&self, path: &Path, is_dir: bool) -> Option<Skip> {
        if self.git_stays.is_some() {
            return self.git_stays.clone();
        }

        let text = path.to_string_lossy().replace('\\', "/");
        let keep = if is_dir {
            gitdir::walks(&text)
        } else {
            gitdir::kind(&text).is_some()
        };
        (!keep).then_some(Skip::Own)
    }

    /// Whether `path`, relative to the project folder, is left out.
    pub(crate) fn leaves_out(&self, path: &Path, is_dir: bool) -> bool {
        self.skip(path, is_dir).is_some()
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
        self.defaults
            .iter()
            .filter_map(|f| f.marker.as_deref())
            .collect()
    }
}

/// The rules of the ignore file `name` in `root`, then the lines of `extra`.
fn read_ignore(root: &Path, name: &str, extra: &str) -> Result<Option<Gitignore>> {
    let path = root.join(name);
    let file = match fs::symlink_metadata(&path) {
        Ok(meta) => meta.is_file(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };

    if !file && extra.trim().is_empty() {
        return Ok(None);
    }

    let mut builder = GitignoreBuilder::new(root);

    if file && let Some(e) = builder.add(&path) {
        log::warn!("some rules in {} were skipped: {e}", path.display());
    }

    for line in extra.lines() {
        if let Err(e) = builder.add_line(None, line) {
            log::warn!("the sync rule {line:?} was skipped: {e}");
        }
    }

    let built = builder
        .build()
        .with_context(|| format!("reading {}", path.display()))?;
    Ok(Some(built))
}

/// The defaults for every marker in `root` or one of its first-level
/// folders, each marker kind once. A marker deeper down brings none.
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
            marker: Some(marker.clone()),
            matcher: builder.build()?,
        });
    }

    let mut builder = GitignoreBuilder::new(root);

    for rule in ALWAYS {
        builder
            .add_line(None, rule)
            .with_context(|| format!("the default rule {rule}"))?;
    }

    found.push(Found {
        marker: None,
        matcher: builder.build()?,
    });
    Ok(found)
}

/// What git tracks in `root`, and every folder above a tracked file; empty
/// when there is no repository to ask yet.
fn tracked(root: &Path) -> HashSet<String> {
    let listed = gitdir::Repo::open(root).map(|repo| repo.tracked());
    let files = match listed {
        Some(Ok(files)) => files,
        Some(Err(e)) => {
            log::debug!("no list of tracked files in {}: {e:#}", root.display());
            return HashSet::new();
        }
        None => return HashSet::new(),
    };
    let mut tracked = HashSet::with_capacity(files.len());

    for file in files {
        let mut folder = file.as_str();

        while let Some(at) = folder.rfind('/') {
            folder = &folder[..at];

            if !tracked.insert(folder.to_owned()) {
                break;
            }
        }

        tracked.insert(file);
    }

    tracked
}

#[cfg(test)]
#[path = "../../tests/files_sync_rules.rs"]
mod tests;
