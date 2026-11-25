//! Where the Beep MCP server's socket lives (`path_for`): in the node's
//! folder when the path fits a Unix socket address, else in
//! `$TMPDIR/beep-<uid>`, made 0700.

use anyhow::Result;
use std::path::{Path, PathBuf};

const SOCKET: &str = "mcp.sock";
/// The longest socket path, in bytes: macOS's `sun_path` holds 104 with
/// the closing NUL, Linux's 108.
#[cfg(unix)]
const MAX_SOCKET_PATH: usize = 103;

/// The socket for the node in `dir`. The node hands sessions the path it
/// bound in `SOCKET_ENV`, so the connecting side never works it out again.
pub(crate) fn path_for(dir: &Path) -> PathBuf {
    path_for_named(dir, SOCKET)
}

/// The socket of the presence listener for the node in `dir`.
pub(crate) fn presence_path(dir: &Path) -> PathBuf {
    path_for_named(dir, "presence.sock")
}

fn path_for_named(dir: &Path, name: &str) -> PathBuf {
    #[cfg(unix)]
    {
        let tmp = std::env::var_os("TMPDIR").map(PathBuf::from);
        choose_named(
            dir,
            tmp.as_deref(),
            rustix::process::getuid().as_raw(),
            name,
        )
    }
    #[cfg(not(unix))]
    {
        // A named pipe's name has no such limit.
        dir.join(name)
    }
}

/// The socket for `dir`: in it when that fits, else a short one named by a
/// hash of `dir` (so two nodes never share it) in `beep-<uid>` under `tmp`,
/// or under `/tmp` when `tmp` is long too.
#[cfg(all(test, unix))]
fn choose(dir: &Path, tmp: Option<&Path>, uid: u32) -> PathBuf {
    choose_named(dir, tmp, uid, SOCKET)
}

#[cfg(unix)]
fn choose_named(dir: &Path, tmp: Option<&Path>, uid: u32, name: &str) -> PathBuf {
    let inside = dir.join(name);

    if fits(&inside) {
        return inside;
    }

    let hash = blake3::hash(dir.as_os_str().as_encoded_bytes()).to_hex();
    let name = format!("{}-{}.sock", name.trim_end_matches(".sock"), &hash[..16]);
    let short = |base: &Path| base.join(format!("beep-{uid}")).join(&name);
    tmp.filter(|tmp| tmp.is_absolute())
        .map(short)
        .filter(|path| fits(path))
        .unwrap_or_else(|| short(Path::new("/tmp")))
}

#[cfg(unix)]
fn fits(path: &Path) -> bool {
    path.as_os_str().len() <= MAX_SOCKET_PATH
}

/// Readies the folder `socket` goes in: the node's own `dir` as it is, or
/// the short per-user folder, made 0700 and refused unless this user owns
/// it and no one else can enter it.
pub(crate) fn prepare(socket: &Path, dir: &Path) -> Result<()> {
    let Some(parent) = socket.parent() else {
        anyhow::bail!("{} has no folder", socket.display());
    };

    if parent == dir {
        return Ok(());
    }
    #[cfg(unix)]
    {
        use anyhow::{Context, bail};
        use std::{
            fs::DirBuilder,
            io::ErrorKind,
            os::unix::fs::{DirBuilderExt, MetadataExt},
        };

        match DirBuilder::new().mode(0o700).create(parent) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e).with_context(|| format!("creating {}", parent.display())),
        }

        let meta = std::fs::symlink_metadata(parent)
            .with_context(|| format!("reading {}", parent.display()))?;
        let uid = rustix::process::getuid().as_raw();

        if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o077 != 0 {
            bail!(
                "{} is not a folder only this user can open",
                parent.display()
            );
        }
    }
    Ok(())
}

#[cfg(all(test, unix))]
#[path = "../tests/agents_mcp_socket.rs"]
mod tests;
