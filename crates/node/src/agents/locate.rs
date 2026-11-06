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
pub(super) fn fallback(program: &str, home: &Path) -> Option<PathBuf> {
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

/// `PATH` with the folder of `program` (found under `home` rather than on
/// `PATH`) in front, so what it starts, such as `node`, is found beside it.
pub(super) fn path_with(program: &Path) -> Option<OsString> {
    let dir = program.parent()?;
    let current = env::var_os("PATH").unwrap_or_default();
    let mut dirs = vec![dir.to_owned()];
    dirs.extend(env::split_paths(&current));
    env::join_paths(dirs).ok()
}

fn folders(home: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![
        // The native installer, then the usual per-user bins.
        home.join(".claude/local"),
        home.join(".local/bin"),
        home.join(".npm-global/bin"),
        home.join(".bun/bin"),
        home.join(".volta/bin"),
        home.join(".asdf/shims"),
        home.join(".local/share/mise/shims"),
        home.join("Library/pnpm"),
        home.join(".local/share/pnpm"),
        home.join("bin"),
    ];
    // Every installed Node of the version managers, newest first.
    for (versions, tail) in [
        (".nvm/versions/node", "bin"),
        (".local/share/fnm/node-versions", "installation/bin"),
        (
            "Library/Application Support/fnm/node-versions",
            "installation/bin",
        ),
    ] {
        dirs.extend(
            newest_first(&home.join(versions))
                .into_iter()
                .map(|dir| dir.join(tail)),
        );
    }
    dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
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
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn install(dir: &Path, program: &str) -> PathBuf {
        fs::create_dir_all(dir).expect("folders");
        let path = dir.join(program);
        fs::write(&path, "#!/bin/sh\n").expect("a program");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("executable");
        path
    }

    #[test]
    fn a_program_is_found_in_the_install_folders_under_home() {
        let home = tempfile::tempdir().expect("a temp folder");
        let name = "beep-test-agent-cli";
        assert_eq!(fallback(name, home.path()), None);
        let local = install(&home.path().join(".local/bin"), name);
        assert_eq!(fallback(name, home.path()), Some(local));
        let bun = install(&home.path().join(".bun/bin"), "beep-test-bun-cli");
        assert_eq!(fallback("beep-test-bun-cli", home.path()), Some(bun));
    }

    #[test]
    fn the_newest_node_version_of_nvm_wins() {
        let home = tempfile::tempdir().expect("a temp folder");
        let versions = home.path().join(".nvm/versions/node");
        install(&versions.join("v9.11.0/bin"), "beep-test-nvm-cli");
        let newest = install(&versions.join("v20.3.1/bin"), "beep-test-nvm-cli");
        assert_eq!(fallback("beep-test-nvm-cli", home.path()), Some(newest));
    }

    #[test]
    fn a_found_folder_goes_first_on_the_path() {
        let path = path_with(Path::new("/home/user/.bun/bin/claude")).expect("a path");
        let first = env::split_paths(&path).next().expect("a folder");
        assert_eq!(first, Path::new("/home/user/.bun/bin"));
    }
}
