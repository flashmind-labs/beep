//! `files/1` (see `beep_proto::node::files`): reads and writes files in the
//! folders shared in `node.toml`, through `Folders::answer`.

use crate::files::paths;
use crate::{
    files::fsat,
    protocol::{Protocol, Session},
};
use anyhow::{Context, Result, bail};
use beep_proto::node::{
    LinkRx, LinkTx,
    files::{FileEntry, FileKind, MAX_CHUNK, PROTOCOL, Reply, Request},
};
use std::{
    fs::{self, Metadata, OpenOptions},
    io::{ErrorKind, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::UNIX_EPOCH,
};
use tokio::task::spawn_blocking;

/// The most entries one `List` returns.
const MAX_ENTRIES: u32 = 1000;

/// The shared folders, resolved.
#[derive(Debug, Clone, Default)]
pub struct Folders {
    roots: Arc<Vec<PathBuf>>,
}

impl Folders {
    /// Resolves `folders`; one that does not exist is left out with a warning.
    pub fn new(folders: &[PathBuf]) -> Self {
        let roots = folders
            .iter()
            .filter_map(|folder| match paths::canonicalize(folder) {
                Ok(root) if root.is_dir() => Some(root),
                Ok(_) => {
                    log::warn!("not sharing {}: not a folder", folder.display());
                    None
                }
                Err(e) => {
                    log::warn!("not sharing {}: {e}", folder.display());
                    None
                }
            })
            .collect();
        Self {
            roots: Arc::new(roots),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    /// The shared folder `resolved`, a path `resolve` gave, lies in.
    pub(crate) fn root_of(&self, resolved: &Path) -> Option<&Path> {
        self.roots
            .iter()
            .map(PathBuf::as_path)
            .find(|root| resolved.starts_with(root))
    }

    /// The first shared folder, which relative paths start in.
    pub fn first(&self) -> Option<&Path> {
        self.roots.first().map(PathBuf::as_path)
    }

    /// The whole disk, with relative paths starting in the first shared
    /// folder, else the home folder. Only for `send_files`: its caller may
    /// run any command on the machine anyway. `files/1` and a transfer asked
    /// for by another machine keep to the shared folders.
    pub(crate) fn everywhere(&self) -> Self {
        let start = self.first().map(Path::to_path_buf).or_else(dirs::home_dir);
        let disks: Vec<PathBuf> = if cfg!(windows) {
            ('A'..='Z')
                .map(|drive| PathBuf::from(format!("{drive}:\\")))
                .filter(|disk| disk.is_dir())
                .collect()
        } else {
            vec![PathBuf::from("/")]
        };
        let roots: Vec<PathBuf> = start.into_iter().chain(disks).collect();
        Self::new(&roots)
    }

    /// `path` with its symlinks resolved, when it lies in a shared folder.
    /// For a path that does not exist yet, its folder must. Resolving
    /// before the check keeps `..` and links from leading out of a shared
    /// folder; a write never follows a link in its last part.
    pub(crate) fn resolve(&self, path: &str) -> Result<PathBuf> {
        let Some(first) = self.roots.first() else {
            bail!("this node shares no folders");
        };
        let path = Path::new(path);
        let path = if path.is_absolute() {
            path.to_owned()
        } else {
            first.join(path)
        };
        let resolved = match paths::canonicalize(&path) {
            Ok(resolved) => resolved,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                let name = match path.components().next_back() {
                    Some(Component::Normal(name)) => name,
                    _ => bail!("{} does not exist", path.display()),
                };
                let parent = path.parent().context("a path needs a folder")?;
                let parent = paths::canonicalize(parent)
                    .with_context(|| format!("{} does not exist", parent.display()))?;
                parent.join(name)
            }
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };

        if !self.roots.iter().any(|root| resolved.starts_with(root)) {
            bail!("{} is outside the shared folders", path.display());
        }

        Ok(resolved)
    }

    /// Answers one request; failures become `Reply::Error`.
    pub fn answer(&self, request: Request) -> Reply {
        let result = match request {
            Request::Roots {} => Ok(Reply::Roots {
                roots: self.roots.iter().map(|root| shown(root)).collect(),
            }),
            Request::Read {
                path,
                offset,
                limit,
            } => self.read(&path, offset, limit),
            Request::Write { path, data, append } => self.write(&path, &data, append),
            Request::List { path, limit } => self.list(&path, limit),
            Request::Stat { path } => self.stat(&path),
            other => Err(anyhow::anyhow!("this node cannot do {other:?} yet")),
        };
        result.unwrap_or_else(|e| Reply::Error {
            message: format!("{e:#}"),
        })
    }

    fn read(&self, path: &str, offset: u64, limit: u32) -> Result<Reply> {
        let path = self.resolve(path)?;
        let mut file =
            fsat::open_read(&path).with_context(|| format!("opening {}", path.display()))?;
        let metadata = file.metadata()?;

        if metadata.is_dir() {
            bail!("{} is a folder", path.display());
        }

        if !metadata.is_file() {
            bail!("{} is not a file", path.display());
        }

        let size = metadata.len();
        let limit = if limit == 0 {
            MAX_CHUNK
        } else {
            limit.min(MAX_CHUNK)
        };
        file.seek(SeekFrom::Start(offset))?;
        let mut data = Vec::new();
        file.take(u64::from(limit)).read_to_end(&mut data)?;
        Ok(Reply::Data {
            path: shown(&path),
            data,
            size,
        })
    }

    fn write(&self, path: &str, data: &[u8], append: bool) -> Result<Reply> {
        if data.len() > MAX_CHUNK as usize {
            bail!("at most {MAX_CHUNK} bytes go in one write");
        }

        let path = self.resolve(path)?;
        let mut options = OpenOptions::new();
        options.create(true);
        if append {
            options.append(true);
        } else {
            options.write(true).truncate(true);
        }

        let mut file = fsat::open_no_follow(&mut options, &path)
            .with_context(|| format!("opening {}", path.display()))?;
        if !file.metadata()?.is_file() {
            bail!("{} is not a file", path.display());
        }

        file.write_all(data)?;
        Ok(Reply::Written {
            path: shown(&path),
            size: file.metadata()?.len(),
        })
    }

    fn list(&self, path: &str, limit: u32) -> Result<Reply> {
        let path = self.resolve(path)?;
        let limit = if limit == 0 {
            MAX_ENTRIES
        } else {
            limit.min(MAX_ENTRIES)
        };
        let mut entries = Vec::new();

        for item in fs::read_dir(&path).with_context(|| format!("listing {}", path.display()))? {
            let item = item?;
            let name = item.file_name().to_string_lossy().into_owned();
            // An entry gone since the listing started is left out.
            if let Ok(metadata) = item.path().symlink_metadata() {
                entries.push(entry(name, &metadata));
            }
        }

        entries.sort_by(|a, b| a.name.cmp(&b.name));
        let more = entries.len().saturating_sub(limit as usize);
        entries.truncate(limit as usize);
        Ok(Reply::Entries {
            path: shown(&path),
            entries,
            more: u32::try_from(more).unwrap_or(u32::MAX),
        })
    }

    fn stat(&self, path: &str) -> Result<Reply> {
        let path = self.resolve(path)?;
        let metadata = path
            .metadata()
            .with_context(|| format!("reading {}", path.display()))?;
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(Reply::Stat {
            path: shown(&path),
            entry: entry(name, &metadata),
        })
    }
}

fn entry(name: String, metadata: &Metadata) -> FileEntry {
    let kind = metadata.file_type();
    let kind = if kind.is_symlink() {
        FileKind::Symlink
    } else if kind.is_dir() {
        FileKind::Dir
    } else if kind.is_file() {
        FileKind::File
    } else {
        FileKind::Other
    };
    FileEntry {
        name,
        kind,
        size: metadata.len(),
        modified: metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|since| since.as_secs()),
    }
}

fn shown(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

impl Protocol for Folders {
    fn name(&self) -> &str {
        PROTOCOL
    }

    fn serve(&self, tx: LinkTx, rx: LinkRx) -> Session {
        Box::pin(serve(tx, rx, self.clone()))
    }
}

/// Serves one `files/1` stream until the caller closes it.
async fn serve(mut tx: LinkTx, mut rx: LinkRx, folders: Folders) -> Result<()> {
    while let Some(request) = rx.recv::<Request>().await? {
        let folders = folders.clone();
        let reply = spawn_blocking(move || folders.answer(request)).await?;
        tx.send(&reply).await?;
    }

    Ok(())
}

#[cfg(test)]
#[path = "../tests/files_folders.rs"]
mod tests;
