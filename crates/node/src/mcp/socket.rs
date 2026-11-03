//! Where the Beep MCP server's socket lives. In the node's folder when the
//! path fits a Unix socket address (104 bytes on macOS, with its NUL);
//! otherwise, as for the Mac app's folder under a long home, in a short
//! folder of this user's own, `$TMPDIR/beep-<uid>`, made 0700. The node
//! hands sessions the path it bound in `SOCKET_ENV`, so the connecting side
//! never works it out again.

use anyhow::Result;
use std::path::{Path, PathBuf};

const SOCKET: &str = "mcp.sock";
/// The longest socket path, in bytes: macOS's `sun_path` holds 104 with
/// the closing NUL, Linux's 108.
#[cfg(unix)]
const MAX_SOCKET_PATH: usize = 103;

/// The socket for the node in `dir`.
pub fn path_for(dir: &Path) -> PathBuf {
    #[cfg(unix)]
    {
        let tmp = std::env::var_os("TMPDIR").map(PathBuf::from);
        choose(dir, tmp.as_deref(), rustix::process::getuid().as_raw())
    }
    #[cfg(not(unix))]
    {
        // A named pipe's name has no such limit.
        dir.join(SOCKET)
    }
}

/// The socket for `dir`: in it when that fits, else a short one named by a
/// hash of `dir` (so two nodes never share it) in `beep-<uid>` under `tmp`,
/// or under `/tmp` when `tmp` is long too.
#[cfg(unix)]
fn choose(dir: &Path, tmp: Option<&Path>, uid: u32) -> PathBuf {
    let inside = dir.join(SOCKET);
    if fits(&inside) {
        return inside;
    }
    let hash = blake3::hash(dir.as_os_str().as_encoded_bytes()).to_hex();
    let name = format!("mcp-{}.sock", &hash[..16]);
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
pub(super) fn prepare(socket: &Path, dir: &Path) -> Result<()> {
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
mod tests {
    use super::*;

    #[test]
    fn a_long_home_gets_a_short_socket_of_its_own() {
        let user = "a".repeat(64);
        let dir = PathBuf::from(format!(
            "/Users/{user}/Library/Application Support/com.flashmind.beep.desktop/node"
        ));
        // macOS's per-user TMPDIR.
        let tmp = Path::new("/var/folders/zz/zyxvpxvq6csfxvn_n0000000000000/T/");
        let socket = choose(&dir, Some(tmp), 501);
        assert!(socket.as_os_str().len() < 104, "{}", socket.display());
        assert!(
            socket.starts_with(tmp.join("beep-501")),
            "{}",
            socket.display()
        );
        let other = dir.with_file_name("other");
        assert_ne!(socket, choose(&other, Some(tmp), 501), "one per node");
        // A TMPDIR as long as the home falls back to /tmp.
        let long_tmp = dir.join("tmp");
        let socket = choose(&dir, Some(&long_tmp), 501);
        assert!(socket.starts_with("/tmp/beep-501"), "{}", socket.display());
        assert!(socket.as_os_str().len() < 104);
        assert!(choose(&dir, None, 501).starts_with("/tmp/beep-501"));
    }

    #[test]
    fn the_short_folder_is_private() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let base = tempfile::tempdir().expect("a temporary folder");
        let socket = base.path().join("beep-1").join("mcp-0.sock");
        prepare(&socket, Path::new("/srv/node")).expect("made");
        let meta = std::fs::metadata(base.path().join("beep-1")).expect("there");
        assert_eq!(meta.mode() & 0o777, 0o700);
        std::fs::set_permissions(
            base.path().join("beep-1"),
            std::fs::Permissions::from_mode(0o755),
        )
        .expect("opened up");
        assert!(prepare(&socket, Path::new("/srv/node")).is_err());
    }
}
