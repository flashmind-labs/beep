//! Finds an agent's program when `PATH` does not have it: an app started
//! from Finder or at login gets a bare `PATH`, and the usual installers put
//! their binaries in a few well-known folders under home.

use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

/// Where `program` is: on `PATH`, or in the usual install folders under
/// `home`. `None` when neither has it.
pub(super) fn find(program: &str, home: Option<&Path>) -> Option<PathBuf> {
    which::which(program)
        .ok()
        .or_else(|| fallback(program, home?))
}

/// `program` in the install folders under `home`, not on `PATH`.
pub(crate) fn fallback(program: &str, home: &Path) -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd"]
    } else {
        &[""]
    };
    folders(home).into_iter().find_map(|dir| {
        names
            .iter()
            .map(|ext| dir.join(format!("{program}{ext}")))
            .find(|path| path.is_file())
    })
}

/// `PATH` with the folders of `programs` (found under `home` or in the
/// node's folder rather than on `PATH`) in front, first first, so what they
/// start, such as `node`, is found beside them.
pub(super) fn path_with(programs: &[&Path]) -> Option<OsString> {
    let current = env::var_os("PATH").unwrap_or_default();
    let mut dirs: Vec<PathBuf> = Vec::new();

    for dir in programs.iter().filter_map(|program| program.parent()) {
        if !dirs.iter().any(|known| known == dir) {
            dirs.push(dir.to_owned());
        }
    }

    if dirs.is_empty() {
        return None;
    }

    dirs.extend(env::split_paths(&current));
    env::join_paths(dirs).ok()
}

/// The per-user install folders under home: the native installer, then the
/// usual per-user bins.
pub(crate) const HOME_BINS: &[&str] = &[
    ".claude/local",
    ".local/bin",
    ".npm-global/bin",
    ".bun/bin",
    ".volta/bin",
    ".asdf/shims",
    ".local/share/mise/shims",
    "Library/pnpm",
    ".local/share/pnpm",
    "bin",
];

/// The folders under home where version managers keep each Node, and the
/// bin folder inside each version.
pub(crate) const NODE_VERSIONS: &[(&str, &str)] = &[
    (".nvm/versions/node", "bin"),
    (".local/share/fnm/node-versions", "installation/bin"),
    (
        "Library/Application Support/fnm/node-versions",
        "installation/bin",
    ),
];

/// The folders of the system's package managers.
pub(crate) const SYSTEM_BINS: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin"];

fn folders(home: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = HOME_BINS.iter().map(|dir| home.join(dir)).collect();
    // Every installed Node of the version managers, newest first.
    for (versions, tail) in NODE_VERSIONS {
        dirs.extend(
            newest_first(&home.join(versions))
                .into_iter()
                .map(|dir| dir.join(tail)),
        );
    }
    dirs.extend(SYSTEM_BINS.iter().map(PathBuf::from));

    if cfg!(windows) {
        dirs.extend(env::var_os("APPDATA").map(|dir| PathBuf::from(dir).join("npm")));
    }

    dirs
}

/// The folders in `versions`, newest version first.
fn newest_first(versions: &Path) -> Vec<PathBuf> {
    let number = |name: &str| -> Vec<u64> {
        name.trim_start_matches('v')
            .split('.')
            .map(|part| part.parse().unwrap_or(0))
            .collect()
    };
    let Ok(entries) = fs::read_dir(versions) else {
        return Vec::new();
    };
    let mut found: Vec<(Vec<u64>, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| Some((number(entry.file_name().to_str()?), entry.path())))
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().map(|(_, path)| path).collect()
}

#[cfg(all(test, unix))]
#[path = "../tests/agents_locate.rs"]
mod tests;
