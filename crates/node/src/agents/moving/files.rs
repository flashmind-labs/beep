//! A chat's or a thread's folder between machines: packed as a tar on the
//! source, without the folders other moving sessions bring themselves, and
//! unpacked on the target. Only files and folders travel; a link stays.

use anyhow::{Context, Result, bail};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};
use tar::{Archive, Builder, EntryType};

/// The most bytes of files one session's folder moves with.
pub(super) const MAX_FILES: u64 = 1024 * 1024 * 1024;

/// Writes what `folder` holds to `out`, leaving out the folders in `apart`.
/// A folder that is gone packs as nothing. Blocks.
pub(super) fn pack(folder: &Path, apart: &[PathBuf], out: impl Write) -> Result<()> {
    let mut tar = Builder::new(Capped {
        out,
        left: MAX_FILES,
    });
    tar.follow_symlinks(false);
    add(&mut tar, folder, Path::new(""), apart)?;
    tar.finish().context("ending the archive")
}

/// How many bytes `pack` writes of `folder`, said before the files go so the
/// target can show how far they are. Reads every file. Blocks.
pub(super) fn packed_size(folder: &Path, apart: &[PathBuf]) -> Result<u64> {
    let mut counted = Counted(0);
    pack(folder, apart, &mut counted)?;
    Ok(counted.0)
}

/// Counts what is written to it.
struct Counted(u64);

impl Write for Counted {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0 += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn add(tar: &mut Builder<impl Write>, dir: &Path, name: &Path, apart: &[PathBuf]) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };

    for entry in entries {
        let entry = entry?;
        let (path, name) = (entry.path(), name.join(entry.file_name()));
        let kind = entry.file_type()?;

        if kind.is_dir() && !apart.contains(&path) {
            tar.append_dir(&name, &path)?;
            add(tar, &path, &name, apart)?;
        } else if kind.is_file() {
            tar.append_path_with_name(&path, &name)
                .with_context(|| format!("packing {}", path.display()))?;
        }
    }

    Ok(())
}

/// Refuses to write more than `left` bytes.
struct Capped<W> {
    out: W,
    left: u64,
}

impl<W: Write> Write for Capped<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let wrote = self.out.write(buf)?;
        self.left = self
            .left
            .checked_sub(wrote as u64)
            .ok_or_else(|| io::Error::other("the chat's files are too large to move"))?;
        Ok(wrote)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

/// Unpacks what `pack` wrote into `folder`, over what is there. The archive
/// comes from another machine: only files and folders inside `folder` are
/// written. Blocks.
pub(super) fn unpack(archive: impl Read, folder: &Path) -> Result<()> {
    fs::create_dir_all(folder)?;

    for entry in Archive::new(archive).entries()? {
        let mut entry = entry?;
        let kind = entry.header().entry_type();

        if !matches!(kind, EntryType::Regular | EntryType::Directory) {
            continue;
        }

        if !entry.unpack_in(folder)? {
            bail!("a file of the chat is outside its folder");
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "../../tests/agents_moving_files.rs"]
mod tests;
