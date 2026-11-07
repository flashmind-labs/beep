//! Chooses a Rust compiler cache for an agent without replacing its settings.

use crate::machine::provision::spawn_env;
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
    let dirs = cwd
        .ancestors()
        .map(|dir| dir.join(".cargo"))
        .chain(cargo_home);
    for dir in dirs {
        for name in ["config", "config.toml"] {
            if configured(&dir.join(name)) {
                return None;
            }
        }
    }
    let path = overrides
        .get("PATH")
        .map(OsString::from)
        .or_else(spawn_env::search_path);
    which::which_in("sccache", path, cwd).ok()
}

/// An unreadable or invalid configuration may contain a wrapper, so leave it alone.
fn configured(path: &Path) -> bool {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return false,
        Err(_) => return true,
    };
    let Ok(config) = text.parse::<toml::Value>() else {
        return true;
    };
    config.get("include").is_some()
        || config
            .get("build")
            .and_then(|build| build.get("rustc-wrapper"))
            .is_some()
        || config.get("env").is_some_and(|vars| {
            ["RUSTC_WRAPPER", "CARGO_BUILD_RUSTC_WRAPPER"]
                .iter()
                .any(|name| vars.get(name).is_some())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_wrapper_and_uncertain_configurations_are_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        assert!(!configured(&config));
        fs::write(&config, "[build]\njobs = 2\n").unwrap();
        assert!(!configured(&config));
        fs::write(&config, "[build]\nrustc-wrapper = 'other'\n").unwrap();
        assert!(configured(&config));
        fs::write(&config, "[env]\nRUSTC_WRAPPER = 'other'\n").unwrap();
        assert!(configured(&config));
        fs::write(&config, "include = 'other.toml'\n").unwrap();
        assert!(configured(&config));
        fs::write(&config, "[invalid").unwrap();
        assert!(configured(&config));
    }
}
