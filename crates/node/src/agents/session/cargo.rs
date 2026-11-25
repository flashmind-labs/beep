//! Shared Cargo outputs and compiler caching without replacing agent settings.

use crate::agents::found;
use crate::machine::spawn_env;
use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

fn variable(name: &str, overrides: &BTreeMap<String, String>) -> Option<OsString> {
    overrides
        .get(name)
        .map(OsString::from)
        .or_else(|| env::var_os(name))
}

/// An absolute executable path, so the wrapper still works if a shell changes PATH.
pub(super) fn wrapper(cwd: &Path, overrides: &BTreeMap<String, String>) -> Option<PathBuf> {
    for name in ["RUSTC_WRAPPER", "CARGO_BUILD_RUSTC_WRAPPER"] {
        if variable(name, overrides).is_some() {
            return None;
        }
    }

    if variable("CARGO_INCREMENTAL", overrides).as_deref() == Some("1".as_ref())
        && variable("SCCACHE_ALLOW_INCREMENTAL", overrides).as_deref() != Some("1".as_ref())
    {
        return None;
    }

    if config_dirs(cwd, overrides).any(|dir| {
        ["config", "config.toml"].iter().any(|name| {
            configured(
                &dir.join(name),
                "rustc-wrapper",
                &["RUSTC_WRAPPER", "CARGO_BUILD_RUSTC_WRAPPER"],
            )
        })
    }) {
        return None;
    }

    let path = overrides
        .get("PATH")
        .map(OsString::from)
        .or_else(spawn_env::search_path);
    which::which_in("sccache", path, cwd).ok()
}

/// Keep worktree builds in the main checkout, even after the agent changes folders.
pub(super) fn target_dir(cwd: &Path, overrides: &BTreeMap<String, String>) -> Option<PathBuf> {
    const VARIABLES: &[&str] = &["CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR"];

    if VARIABLES
        .iter()
        .any(|name| variable(name, overrides).is_some())
    {
        return None;
    }

    let target = project_target(cwd)?;

    if config_dirs(cwd, overrides).any(|dir| {
        ["config", "config.toml"]
            .iter()
            .any(|name| configured(&dir.join(name), "target-dir", VARIABLES))
    }) {
        return None;
    }

    Some(target)
}

fn project_target(cwd: &Path) -> Option<PathBuf> {
    let cwd = dunce::canonicalize(cwd).ok()?;
    let repository = found::repository(&cwd);
    let top = repository.as_ref().and_then(|repo| repo.workdir());
    let manifest = cwd
        .ancestors()
        .take_while(|dir| top.is_none_or(|top| dir.starts_with(top)))
        .find(|dir| dir.join("Cargo.toml").is_file())?;
    let root = found::checkout(&cwd).map_or_else(|| manifest.to_owned(), |checkout| checkout.root);

    dunce::canonicalize(root)
        .ok()
        .map(|root| root.join("target"))
}

fn config_dirs<'a>(
    cwd: &'a Path,
    overrides: &BTreeMap<String, String>,
) -> impl Iterator<Item = PathBuf> + 'a {
    let cargo_home = variable("CARGO_HOME", overrides)
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            }
        })
        .or_else(|| dirs::home_dir().map(|home| home.join(".cargo")));
    cwd.ancestors()
        .map(|dir| dir.join(".cargo"))
        .chain(cargo_home)
}

/// An unreadable or invalid configuration may contain a setting, so leave it alone.
fn configured(path: &Path, key: &str, variables: &[&str]) -> bool {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return false,
        Err(_) => return true,
    };
    let Ok(config) = toml::from_str::<toml::Value>(&text) else {
        return true;
    };
    config.get("include").is_some()
        || config
            .get("build")
            .and_then(|build| build.get(key))
            .is_some()
        || config
            .get("env")
            .is_some_and(|vars| variables.iter().any(|name| vars.get(name).is_some()))
}

#[cfg(test)]
#[path = "../../tests/agents_session_cargo.rs"]
mod tests;
