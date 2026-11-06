//! Paths as every Windows program reads them. `fs::canonicalize` gives a
//! verbatim path there (`\\?\C:\x`), which Git for Windows and many other
//! programs refuse, so the node never keeps or passes one on.

use std::{
    io,
    path::{Path, PathBuf},
};

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
mod tests {
    use super::*;

    #[test]
    fn verbatim_paths_become_plain() {
        assert_eq!(plain(Path::new(r"\\?\C:\a")), Path::new(r"C:\a"));
        assert_eq!(
            plain(Path::new(r"\\?\UNC\srv\share\a")),
            Path::new(r"\\srv\share\a")
        );
        assert_eq!(plain(Path::new("/home/me/a")), Path::new("/home/me/a"));
        assert_eq!(plain(Path::new(r"C:\a")), Path::new(r"C:\a"));
        assert_eq!(
            plain(Path::new(r"\\?\Volume{abc}\a")),
            Path::new(r"\\?\Volume{abc}\a")
        );
        let long = format!(r"\\?\C:\{}", "a".repeat(MAX_PATH));
        assert_eq!(plain(Path::new(&long)), Path::new(&long));
    }
}
