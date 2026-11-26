//! Paths as every Windows program reads them. `fs::canonicalize` gives a
//! verbatim path there (`\\?\C:\x`), which Git for Windows and many other
//! programs refuse, so the node never keeps or passes one on.

use anyhow::{Context, Result, bail};
use std::{
    io::{self, ErrorKind},
    path::{Component, Path, PathBuf},
};

/// `path`, absolute, with its links and `..` resolved. A path not there
/// yet is its nearest folder that is, resolved, with the rest joined.
/// Blocks on the file system.
pub(crate) fn real(path: &Path) -> Result<PathBuf> {
    let mut missing = Vec::new();
    let mut at = path;

    loop {
        match canonicalize(at) {
            Ok(mut found) => {
                for name in missing.iter().rev() {
                    found.push(name);
                }

                return Ok(found);
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {
                let mut parts = at.components();

                match parts.next_back() {
                    Some(Component::Normal(name)) => missing.push(name.to_owned()),
                    _ => bail!("{} leads nowhere it can be checked", path.display()),
                }
                at = parts.as_path();
            }
            Err(e) => return Err(e).with_context(|| format!("could not open {}", path.display())),
        }
    }
}

/// The longest path the plain Windows APIs take.
const MAX_PATH: usize = 260;

/// `path` resolved as `fs::canonicalize` does, spelled `C:\x` or
/// `\\host\share\x` rather than verbatim.
pub(crate) fn canonicalize(path: impl AsRef<Path>) -> io::Result<PathBuf> {
    dunce::canonicalize(path).map(|real| plain(&real))
}

/// `path` without its verbatim prefix, when what is left names the same
/// place: `\\?\C:\x` is `C:\x` and `\\?\UNC\host\share` is `\\host\share`.
/// Any other path, and one too long for the plain form, is kept as it is.
pub(crate) fn plain(path: &Path) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path.to_owned();
    };
    let short = if let Some(share) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{share}")
    } else if let Some(rest) = text.strip_prefix(r"\\?\")
        && rest.as_bytes().get(1) == Some(&b':')
        && rest.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
    {
        rest.to_owned()
    } else {
        return path.to_owned();
    };
    if short.len() < MAX_PATH {
        PathBuf::from(short)
    } else {
        path.to_owned()
    }
}

#[cfg(test)]
#[path = "../tests/fs_paths.rs"]
mod tests;
