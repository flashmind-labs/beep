//! Splitting a branch's diff between reviewers. A small change gets one
//! reviewer; a bigger one is grouped by area (a crate, a package, an app
//! folder), each file kept with its tests, into shards of about
//! [`SHARD_LINES`], at most [`MAX_SHARDS`]. Lockfiles and files the repo
//! marks `linguist-generated` are not reviewed.

use super::git::Changed;
use std::collections::{BTreeMap, BTreeSet};

/// Below this many reviewed lines, one reviewer reads it all.
pub(crate) const SINGLE_LINES: u32 = 800;
/// About how many lines one shard holds.
pub(crate) const SHARD_LINES: u32 = 600;
/// The most shards a review has.
pub(crate) const MAX_SHARDS: usize = 4;
/// The most areas a shard's name lists.
const NAMED_AREAS: usize = 2;

/// Folders whose children are areas of their own: `crates/core`, not
/// `crates`.
const AREA_PARENTS: [&str; 9] = [
    "crates", "apps", "packages", "libs", "services", "cmd", "internal", "modules", "plugins",
];

const LOCKFILES: [&str; 14] = [
    "Cargo.lock",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lockb",
    "bun.lock",
    "Gemfile.lock",
    "poetry.lock",
    "uv.lock",
    "composer.lock",
    "go.sum",
    "flake.lock",
    "Package.resolved",
];

/// One reviewer's part of the diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Shard {
    pub(crate) area: String,
    pub(crate) paths: Vec<String>,
    pub(crate) lines: u32,
}

/// The files to review, split between reviewers, biggest shard first.
/// Empty when only lockfiles and generated files changed.
pub(crate) fn shard(changed: &[Changed], gitattributes: &str) -> Vec<Shard> {
    let generated = Generated::parse(gitattributes);
    let kept: Vec<&Changed> = changed
        .iter()
        .filter(|file| !is_lockfile(&file.path) && !generated.matches(&file.path))
        .collect();
    if kept.is_empty() {
        return Vec::new();
    }
    let total: u32 = kept.iter().map(|file| file.lines).sum();
    if total < SINGLE_LINES {
        return vec![shard_of(&[Unit::of(".", kept)])];
    }
    let mut areas: BTreeMap<String, Vec<&Changed>> = BTreeMap::new();
    for file in kept {
        areas.entry(area_key(&file.path)).or_default().push(file);
    }
    let mut units: Vec<Unit<'_>> = Vec::new();
    for (area, files) in areas {
        let lines: u32 = files.iter().map(|file| file.lines).sum();
        if lines <= SHARD_LINES {
            units.push(Unit::of(&area, files));
            continue;
        }
        // Too big for one shard: by file, each with its tests.
        let mut by_stem: BTreeMap<String, Vec<&Changed>> = BTreeMap::new();
        for file in files {
            by_stem.entry(stem_key(&file.path)).or_default().push(file);
        }
        units.extend(by_stem.into_values().map(|files| Unit::of(&area, files)));
    }
    let wanted = usize::try_from(total.div_ceil(SHARD_LINES))
        .unwrap_or(MAX_SHARDS)
        .clamp(1, MAX_SHARDS);
    // Biggest first, each into the emptiest bin: a unit bigger than a
    // shard lands alone in an empty one.
    units.sort_by(|a, b| b.lines.cmp(&a.lines).then(a.area.cmp(&b.area)));
    let mut bins: Vec<Vec<Unit<'_>>> = (0..wanted).map(|_| Vec::new()).collect();
    for unit in units {
        if let Some(bin) = bins
            .iter_mut()
            .min_by_key(|bin| bin.iter().map(|unit| unit.lines).sum::<u32>())
        {
            bin.push(unit);
        }
    }
    let mut out: Vec<Shard> = bins
        .into_iter()
        .filter(|bin| !bin.is_empty())
        .map(|bin| shard_of(&bin))
        .collect();
    out.sort_by(|a, b| b.lines.cmp(&a.lines).then(a.area.cmp(&b.area)));
    out
}

/// Files that go to one reviewer together.
struct Unit<'a> {
    area: String,
    files: Vec<&'a Changed>,
    lines: u32,
}

impl<'a> Unit<'a> {
    fn of(area: &str, files: Vec<&'a Changed>) -> Self {
        Unit {
            area: area.to_owned(),
            lines: files.iter().map(|file| file.lines).sum(),
            files,
        }
    }
}

fn shard_of(units: &[Unit<'_>]) -> Shard {
    let mut paths: Vec<String> = units
        .iter()
        .flat_map(|unit| unit.files.iter().map(|file| file.path.clone()))
        .collect();
    paths.sort();
    // Each area once, in the order its units came; a split area's units
    // need not sit next to each other.
    let mut seen = BTreeSet::new();
    let areas: Vec<&str> = units
        .iter()
        .map(|unit| unit.area.as_str())
        .filter(|area| seen.insert(*area))
        .collect();
    let area = match areas.as_slice() {
        [one] if *one != "." => (*one).to_owned(),
        _ => {
            let common = common_area(&paths);
            if common != "." || areas.len() <= 1 {
                common
            } else if areas.len() <= NAMED_AREAS {
                areas.join(", ")
            } else {
                format!(
                    "{} and {} more",
                    areas[..NAMED_AREAS].join(", "),
                    areas.len() - NAMED_AREAS
                )
            }
        }
    };
    Shard {
        area,
        lines: units.iter().map(|unit| unit.lines).sum(),
        paths,
    }
}

/// The area a path belongs to: `crates/core` for
/// `crates/core/src/lib.rs`, `docs` for `docs/a.md`, `.` for a file at the
/// top.
fn area_key(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    match parts.as_slice() {
        [_] => ".".to_owned(),
        [parent, child, _, ..] if AREA_PARENTS.contains(parent) => format!("{parent}/{child}"),
        [first, ..] => (*first).to_owned(),
        [] => ".".to_owned(),
    }
}

/// The folders all `paths` share, at most two deep, or `.` for none.
fn common_area(paths: &[String]) -> String {
    let mut common: Option<Vec<&str>> = None;
    for path in paths {
        let mut here: Vec<&str> = path.split('/').collect();
        here.pop();
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

/// A file's name without its folder, extension and test markers, so
/// `src/limit.rs`, `tests/limit.rs` and `limit.test.ts` meet.
fn stem_key(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    let stem = name.split('.').next().unwrap_or(name);
    let stem = stem
        .strip_prefix("test_")
        .or_else(|| stem.strip_suffix("_test"))
        .or_else(|| stem.strip_suffix("_tests"))
        .or_else(|| stem.strip_suffix("_spec"))
        .unwrap_or(stem);
    stem.to_ascii_lowercase()
}

fn is_lockfile(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    LOCKFILES.contains(&name)
}

/// The `linguist-generated` lines of a `.gitattributes`.
struct Generated {
    /// Pattern, and whether it sets or unsets the attribute. The last
    /// matching line wins.
    rules: Vec<(String, bool)>,
}

impl Generated {
    fn parse(text: &str) -> Self {
        let mut rules = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut words = line.split_whitespace();
            let Some(pattern) = words.next() else {
                continue;
            };
            for attr in words {
                let set = match attr {
                    "linguist-generated" | "linguist-generated=true" => true,
                    "-linguist-generated" | "!linguist-generated" | "linguist-generated=false" => {
                        false
                    }
                    _ => continue,
                };
                rules.push((pattern.to_owned(), set));
            }
        }
        Generated { rules }
    }

    fn matches(&self, path: &str) -> bool {
        self.rules
            .iter()
            .rev()
            .find(|(pattern, _)| pattern_matches(pattern, path))
            .is_some_and(|(_, set)| *set)
    }
}

/// Whether a `.gitattributes` pattern names `path`: one without a slash
/// matches the file name anywhere, one with a slash the whole path.
fn pattern_matches(pattern: &str, path: &str) -> bool {
    match pattern.strip_prefix('/') {
        Some(anchored) => glob(anchored.as_bytes(), path.as_bytes()),
        None if pattern.contains('/') => glob(pattern.as_bytes(), path.as_bytes()),
        None => {
            let name = path.rsplit('/').next().unwrap_or(path);
            glob(pattern.as_bytes(), name.as_bytes())
        }
    }
}

/// `*` and `?` stay inside one folder; `**` crosses folders.
fn glob(pattern: &[u8], text: &[u8]) -> bool {
    match pattern {
        [] => text.is_empty(),
        [b'*', b'*', rest @ ..] => {
            let rest = rest.strip_prefix(b"/").unwrap_or(rest);
            (0..=text.len()).any(|at| glob(rest, &text[at..]))
        }
        [b'*', rest @ ..] => {
            let mut at = 0;
            loop {
                if glob(rest, &text[at..]) {
                    return true;
                }
                if at == text.len() || text[at] == b'/' {
                    return false;
                }
                at += 1;
            }
        }
        [b'?', rest @ ..] => {
            matches!(text.first(), Some(c) if *c != b'/') && glob(rest, &text[1..])
        }
        [c, rest @ ..] => text.first() == Some(c) && glob(rest, &text[1..]),
    }
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

    fn holding<'a>(shards: &'a [Shard], path: &str) -> &'a Shard {
        shards
            .iter()
            .find(|shard| shard.paths.iter().any(|p| p == path))
            .expect("the path is in a shard")
    }

    #[test]
    fn shards_keep_tests_with_code() {
        let files = [
            changed("crates/core/src/limit.rs", 450),
            changed("crates/core/tests/limit.rs", 150),
            changed("crates/core/src/session.rs", 400),
            changed("apps/web/src/Login.tsx", 300),
            changed("apps/web/src/Login.test.tsx", 100),
        ];
        let got = shard(&files, "");
        assert!(got.len() > 1, "{got:?}");
        assert_eq!(
            holding(&got, "crates/core/src/limit.rs"),
            holding(&got, "crates/core/tests/limit.rs")
        );
        assert_eq!(
            holding(&got, "apps/web/src/Login.tsx"),
            holding(&got, "apps/web/src/Login.test.tsx")
        );
        let total: u32 = got.iter().map(|s| s.lines).sum();
        assert_eq!(total, 1400);
    }

    #[test]
    fn oversized_file_is_own_shard() {
        let files = [
            changed("crates/core/src/big.rs", 1500),
            changed("crates/core/src/small.rs", 40),
            changed("apps/web/src/a.ts", 200),
            changed("docs/notes.md", 60),
        ];
        let got = shard(&files, "");
        let big = holding(&got, "crates/core/src/big.rs");
        assert_eq!(big.paths, ["crates/core/src/big.rs"]);
        assert_eq!(big.area, "crates/core");
        assert!(got.len() <= MAX_SHARDS);
    }

    #[test]
    fn generated_files_skipped() {
        let attrs = "# generated\n\
            gen/** linguist-generated\n\
            *.pb.go linguist-generated=true\n\
            gen/keep.rs -linguist-generated\n";
        let files = [
            changed("Cargo.lock", 4000),
            changed("apps/web/package-lock.json", 900),
            changed("gen/api.rs", 2000),
            changed("gen/keep.rs", 30),
            changed("internal/rpc/service.pb.go", 700),
            changed("src/main.rs", 50),
        ];
        let got = shard(&files, attrs);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].paths, ["gen/keep.rs", "src/main.rs"]);
        assert!(shard(&[changed("Cargo.lock", 10)], "").is_empty());
    }

    #[test]
    fn at_most_four_shards() {
        let files: Vec<Changed> = (0..10)
            .map(|n| changed(&format!("dir{n}/file.rs"), 300))
            .collect();
        let got = shard(&files, "");
        assert_eq!(got.len(), MAX_SHARDS);
        assert_eq!(got.iter().map(|s| s.paths.len()).sum::<usize>(), 10);
        assert!(got.iter().all(|s| s.lines <= 900), "{got:?}");
        assert!(
            got.iter()
                .any(|s| s.area.contains(" more") || s.area.contains(", "))
        );
    }
}
