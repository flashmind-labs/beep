//! Putting a new binary in place of the running one.

use super::types::Error;
use crate::files::paths;
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, ErrorKind, Write},
    path::{Path, PathBuf},
};

/// Where this binary is.
pub(super) fn this_binary() -> Result<PathBuf, Error> {
    env::current_exe()
        .and_then(paths::canonicalize)
        .map_err(|source| Error::Io {
            action: "finding",
            path: PathBuf::from("this binary"),
            source,
        })
}

/// Maps an I/O error on `path`, telling a missing permission apart.
fn io_error(action: &'static str, path: &Path) -> impl FnOnce(io::Error) -> Error {
    move |source| {
        if source.kind() == ErrorKind::PermissionDenied {
            let dir = path.parent().unwrap_or(path).to_owned();
            return Error::NoPermission { dir };
        }

        Error::Io {
            action,
            path: path.to_owned(),
            source,
        }
    }
}

/// The file the new binary is written to before it replaces `exe`.
fn staged(exe: &Path) -> PathBuf {
    let name = exe
        .file_name()
        .map_or_else(|| "beep-node".into(), |name| name.to_string_lossy());
    exe.with_file_name(format!(".{name}.new"))
}

/// Whether a file can be written next to `exe`.
pub(super) fn writable(exe: &Path) -> Result<(), Error> {
    let path = staged(exe);
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .map_err(io_error("writing", &path))?;
    fs::remove_file(&path).map_err(io_error("removing", &path))
}

/// Puts `binary` in place of `exe` in one rename, so nothing ever runs a
/// half-written file. It keeps `exe`'s permissions.
pub(super) fn replace(exe: &Path, binary: &[u8]) -> Result<(), Error> {
    let fresh = staged(exe);
    let written = write_fresh(exe, &fresh, binary);
    let swapped = written.and_then(|()| swap(exe, &fresh));

    if swapped.is_err() {
        // Best effort: what matters is the error that stopped the update.
        let _ = fs::remove_file(&fresh);
    }

    swapped
}

fn write_fresh(exe: &Path, fresh: &Path, binary: &[u8]) -> Result<(), Error> {
    let permissions = fs::metadata(exe)
        .map_err(io_error("reading", exe))?
        .permissions();
    let mut file = File::create(fresh).map_err(io_error("writing", fresh))?;
    file.write_all(binary)
        .and_then(|()| file.sync_all())
        .map_err(io_error("writing", fresh))?;
    drop(file);
    fs::set_permissions(fresh, permissions).map_err(io_error("setting permissions on", fresh))
}

/// Renames `fresh` over `exe`.
#[cfg(not(windows))]
fn swap(exe: &Path, fresh: &Path) -> Result<(), Error> {
    fs::rename(fresh, exe).map_err(io_error("replacing", exe))
}

/// Renames `fresh` over `exe`. A running `.exe` cannot be overwritten but
/// can be renamed, so the old one moves aside first.
#[cfg(windows)]
fn swap(exe: &Path, fresh: &Path) -> Result<(), Error> {
    swap_aside(exe, fresh, &aside(exe))
}

/// Where the old binary waits: `beep-node.exe.old`, the name `install.ps1`
/// uses too. It stays until the next update removes it.
#[cfg(any(windows, test))]
fn aside(exe: &Path) -> PathBuf {
    let mut name = exe.as_os_str().to_owned();
    name.push(".old");
    PathBuf::from(name)
}

/// Moves `exe` to `old`, then `fresh` to `exe`, putting `exe` back when the
/// second rename fails. Plain renames, so it runs anywhere; only Windows
/// needs it.
#[cfg(any(windows, test))]
fn swap_aside(exe: &Path, fresh: &Path, old: &Path) -> Result<(), Error> {
    match fs::remove_file(old) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(source) => {
            return Err(Error::Io {
                action: "removing the copy an earlier update left (is a beep-node still running from it?)",
                path: old.to_owned(),
                source,
            });
        }
    }

    fs::rename(exe, old).map_err(io_error("moving aside", exe))?;

    if let Err(e) = fs::rename(fresh, exe) {
        // Best effort: puts the old binary back, so the machine keeps a
        // beep-node; the error that matters is the failed rename.
        let _ = fs::rename(old, exe);
        return Err(io_error("replacing", exe)(e));
    }

    Ok(())
}

#[cfg(test)]
#[path = "../../tests/machine_update_install.rs"]
mod tests;
