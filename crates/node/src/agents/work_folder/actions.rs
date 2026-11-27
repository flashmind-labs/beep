//! File browser mutations, anchored to open directories and never overwriting a destination.

use super::{PART, WorkRoot};
use crate::fs::fsat::{self, Dir};
use anyhow::{Context, Result, bail};
use beep_proto::node::agent::{FileAction, RelPath};
use std::{
    collections::HashSet,
    ffi::{OsStr, OsString},
    io::{self, Write},
    path::{Path, PathBuf},
};

const MAX_ITEMS: usize = 50_000;
const MAX_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Default)]
struct CopyBudget {
    count: usize,
    bytes: u64,
}

fn checked(path: &RelPath) -> Result<PathBuf> {
    if path.is_root() || path.parts().any(|p| p.eq_ignore_ascii_case(".git")) {
        bail!("The working folder and Git metadata cannot be changed here");
    }

    Ok(path.into())
}

fn folder(root: &WorkRoot, rel: &Path) -> Result<Dir> {
    let mut dir = Dir::open(&root.root)?;

    for part in rel.components() {
        dir = dir.open_dir(part.as_os_str())?;
    }

    Ok(dir)
}

fn parent(root: &WorkRoot, path: &Path) -> Result<Dir> {
    folder(root, path.parent().context("A file name is required")?)
}

fn name(path: &Path) -> Result<&OsStr> {
    path.file_name().context("A file name is required")
}

/// Removes descendants when their parent is selected, preserving selection order.
fn roots(paths: &[RelPath]) -> Result<Vec<PathBuf>> {
    if paths.is_empty() || paths.len() > MAX_ITEMS {
        bail!("Select between 1 and {MAX_ITEMS} items");
    }

    let all = paths.iter().map(checked).collect::<Result<Vec<_>>>()?;
    let lookup: HashSet<_> = all.iter().cloned().collect();
    let mut seen = HashSet::new();
    Ok(all
        .iter()
        .filter(|p| !p.ancestors().skip(1).any(|parent| lookup.contains(parent)))
        .filter(|p| seen.insert((*p).clone()))
        .cloned()
        .collect())
}

/// Checks a copy before creating anything. Links and special files are refused.
fn measure(
    dir: &Dir,
    entry: &OsStr,
    count: &mut usize,
    bytes: &mut u64,
    depth: usize,
) -> Result<()> {
    if depth > 128 || entry.eq_ignore_ascii_case(".git") {
        bail!("Cannot copy Git metadata or folders nested over 128 levels");
    }
    *count += 1;

    if *count > MAX_ITEMS {
        bail!("Copy at most {MAX_ITEMS} items at once");
    }

    let meta = dir.metadata(entry)?.context("An item is no longer there")?;
    if meta.is_dir() {
        let child = dir.open_dir(entry)?;

        for name in child.entries()? {
            measure(&child, &name, count, bytes, depth + 1)?;
        }
    } else if meta.is_file() {
        *bytes = bytes
            .checked_add(meta.len())
            .context("The copy is too large")?;
        if *bytes > MAX_BYTES {
            bail!("Copy at most 256 MB at once");
        }
    } else {
        bail!("Only regular files and folders can be copied");
    }

    Ok(())
}

fn copy_entry(
    from: &Dir,
    source: &OsStr,
    to: &Dir,
    dest: &OsStr,
    budget: &mut CopyBudget,
    depth: usize,
) -> Result<()> {
    // Repeat the bounds while copying: another process may have changed the source.
    if depth > 128 || source.eq_ignore_ascii_case(".git") {
        bail!("Cannot copy this folder");
    }

    budget.count += 1;

    if budget.count > MAX_ITEMS {
        bail!("Too many items to copy");
    }

    let meta = from
        .metadata(source)?
        .context("An item is no longer there")?;
    if meta.is_dir() {
        let src = from.open_dir(source)?;
        to.make_dir(dest, false)?;
        let dst = to.open_dir(dest)?;

        for entry in src.entries()? {
            copy_entry(&src, &entry, &dst, &entry, budget, depth + 1)?;
        }
    } else if meta.is_file() {
        let mut src = from.read_file(source)?;
        let mut dst = to.create_file(dest, fsat::is_executable(&meta))?;
        use std::io::Read;
        let copied = io::copy(
            &mut (&mut src).take(MAX_BYTES.saturating_sub(budget.bytes) + 1),
            &mut dst,
        )?;
        budget.bytes = budget
            .bytes
            .checked_add(copied)
            .context("The copy is too large")?;
        if budget.bytes > MAX_BYTES {
            bail!("Copy at most 256 MB at once");
        }
    } else {
        bail!("Only regular files and folders can be copied");
    }

    Ok(())
}

pub(in crate::agents) fn edit(root: &WorkRoot, action: &FileAction) -> Result<()> {
    match action {
        FileAction::CreateFile { path } | FileAction::CreateFolder { path } => {
            let path = checked(path)?;
            let dir = parent(root, &path)?;
            if matches!(action, FileAction::CreateFolder { .. }) {
                dir.make_dir(name(&path)?, false)?;
            } else {
                dir.create_file(name(&path)?, false)?;
            }
        }
        FileAction::Rename { path, to } => {
            let (path, to) = (checked(path)?, checked(to)?);

            if to.starts_with(&path) {
                bail!("An item cannot be moved into itself");
            }

            let (src, dst) = (parent(root, &path)?, parent(root, &to)?);
            let meta = src
                .metadata(name(&path)?)?
                .context("The item is no longer there")?;
            fsat::rename_new(&src, name(&path)?, &dst, name(&to)?, meta.is_dir())?;
        }
        FileAction::Copy {
            paths,
            folder: dest,
        }
        | FileAction::Move {
            paths,
            folder: dest,
        } => {
            let paths = roots(paths)?;
            let dest = if dest.is_root() {
                PathBuf::new()
            } else {
                checked(dest)?
            };
            let dst = folder(root, &dest)?;
            let mut taken = HashSet::new();
            let (mut count, mut bytes) = (0, 0);
            // Validate the whole selection before the first mutation.
            for path in &paths {
                let entry = name(path)?;

                if dest.starts_with(path) {
                    bail!("A folder cannot be placed inside itself");
                }

                if !taken.insert(entry.to_owned()) || dst.metadata(entry)?.is_some() {
                    bail!(
                        "{} already exists in the destination",
                        entry.to_string_lossy()
                    );
                }

                let src = parent(root, path)?;
                src.metadata(entry)?.context("An item is no longer there")?;

                if matches!(action, FileAction::Copy { .. }) {
                    measure(&src, entry, &mut count, &mut bytes, 0)?;
                }
            }

            let mut budget = CopyBudget::default();

            for path in &paths {
                let src = parent(root, path)?;
                let entry = name(path)?;
                let result = if matches!(action, FileAction::Copy { .. }) {
                    copy_entry(&src, entry, &dst, entry, &mut budget, 0)
                } else {
                    let meta = src.metadata(entry)?.context("An item is no longer there")?;
                    fsat::rename_new(&src, entry, &dst, entry, meta.is_dir()).map_err(Into::into)
                };
                result.with_context(|| format!("Could not transfer {}. Some items may be incomplete or already transferred; refresh before retrying", path.display()))?;
            }
        }
        FileAction::Delete { paths } => {
            let paths = roots(paths)?;

            for path in &paths {
                parent(root, path)?;
            }

            for path in &paths {
                parent(root, path)?
                    .remove_tree(name(path)?)
                    .with_context(|| {
                        format!(
                            "Could not delete {}. Earlier items may have been deleted",
                            path.display()
                        )
                    })?;
            }
        }
        FileAction::Write { path, text } => {
            let path = checked(path)?;
            let (dir, file) = (parent(root, &path)?, name(&path)?);
            replace(&dir, file, text.as_bytes())
                .with_context(|| format!("Could not save {}", path.display()))?;
        }
    }

    Ok(())
}

/// Writes `data` beside the regular file `file` with its permissions, then
/// moves it over the file, so a reader sees the old text or the new.
fn replace(dir: &Dir, file: &OsStr, data: &[u8]) -> Result<()> {
    let meta = dir
        .metadata(file)
        .context("Links cannot be edited here")?
        .filter(|meta| meta.is_file())
        .context("It is no longer a file there")?;
    let mut temp = OsString::from(".");
    temp.push(file);
    temp.push(PART);

    // A save that stopped part way left its part.
    match dir.remove_file(&temp) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }

    let written = dir.create_file(&temp, false).and_then(|mut out| {
        out.write_all(data)?;
        out.set_permissions(meta.permissions())?;
        out.sync_all()
    });
    let moved = written.and_then(|()| fsat::rename(dir, &temp, dir, file));

    if let Err(e) = moved {
        let _ = dir.remove_file(&temp);
        return Err(e.into());
    }

    Ok(())
}

#[cfg(test)]
#[path = "../../tests/agents_work_folder_actions.rs"]
mod tests;
