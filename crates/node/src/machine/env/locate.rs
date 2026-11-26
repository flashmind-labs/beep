//! Finds an agent's program when `PATH` does not have it: an app started
//! from Finder or at login gets a bare `PATH`, and the usual installers put
//! their binaries in a few well-known folders under home.

use anyhow::{Context, Result};
use std::{
    env,
    ffi::OsString,
    fs,
    path::{Component, Path, PathBuf},
};

/// Where `program` is: on `PATH`, or in the usual install folders under
/// `home`. `None` when neither has it.
pub(crate) fn find(program: &str, home: Option<&Path>) -> Option<PathBuf> {
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
pub(crate) fn path_with(programs: &[&Path]) -> Option<OsString> {
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

/// The per-user install folders under home: the native installers, then the
/// usual per-user bins.
pub(crate) const HOME_BINS: &[&str] = &[
    ".claude/local",
    "AppData/Local/agy/bin",
    ".local/bin",
    ".cargo/bin",
    "go/bin",
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
#[path = "../../tests/machine_env_locate.rs"]
mod tests;

/// The folder a session runs in: `cwd`, with `~` for the home folder, or
/// the home folder itself.
pub(crate) fn folder(cwd: Option<&str>) -> Result<PathBuf> {
    let home = dirs::home_dir();
    let path = match cwd.map(str::trim) {
        None | Some("") | Some("~") => home.clone().context("this machine has no home folder")?,
        Some(rest) if rest.starts_with("~/") => home
            .as_deref()
            .context("this machine has no home folder")?
            .join(&rest[2..]),
        Some(path) => PathBuf::from(path),
    };
    anyhow::ensure!(
        path.is_absolute(),
        "the folder must be an absolute path, not {}",
        path.display()
    );

    if path.is_dir() {
        return Ok(path);
    }
    // A folder named on another machine: `/Users/<name>/x` there is
    // `~/x` here, and `~/Projects/x` there is often `~/x` here and the other
    // way round, as project groups match them.
    if let Some(home) = home.as_deref()
        && let Some(found) = home_twins(&path, home).into_iter().find(|p| p.is_dir())
    {
        return Ok(found);
    }

    anyhow::bail!("there is no folder {}", path.display())
}

/// Where `path`, named under some home folder, may be under `home`: the
/// same place, then with a leading `Projects/` taken away or added.
pub(crate) fn home_twins(path: &Path, home: &Path) -> Vec<PathBuf> {
    let rest = path.strip_prefix(home).ok().or_else(|| {
        let mut parts = path.components();
        let root = parts.next()?;
        let top = parts.next()?;
        let under = matches!(top.as_os_str().to_str(), Some("Users" | "home"));
        (matches!(root, Component::RootDir) && under && parts.next().is_some())
            .then_some(parts.as_path())
    });
    let Some(rest) = rest else {
        return Vec::new();
    };
    let toggled = match rest.strip_prefix("Projects") {
        Ok(inner) if !inner.as_os_str().is_empty() => home.join(inner),
        _ => home.join("Projects").join(rest),
    };
    [home.join(rest), toggled]
        .into_iter()
        .filter(|p| p != path)
        .collect()
}
