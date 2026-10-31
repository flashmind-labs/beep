//! Copying a file from one machine to another over their `files/1`, a
//! chunk at a time, so a large file is never held whole.

use super::Picked;
use crate::{
    nodes::Online,
    protocol::node::files::{FileKind, MAX_CHUNK, Reply, Request},
};
use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use std::{path::Path, time::Duration};

/// The largest file a copy takes.
pub(crate) const MAX_COPY: u64 = 2 << 30;
/// How long making the destination's folder may take.
const MKDIR_LIMIT: Duration = Duration::from_secs(30);

/// A machine's files, as a copy uses them.
#[async_trait]
pub(crate) trait FileNode: Sync {
    async fn files(&self, request: &Request) -> Result<Reply>;
    /// Makes `dir` and its parents, if the machine can.
    async fn make_dirs(&self, dir: &str) -> Result<()>;
    fn same_machine(&self, other: &Self) -> bool;
}

#[async_trait]
impl FileNode for Online {
    async fn files(&self, request: &Request) -> Result<Reply> {
        Online::files(self, request).await
    }

    async fn make_dirs(&self, dir: &str) -> Result<()> {
        if !self.offers(crate::nodes::SHELL) {
            // Without a shell the folder must exist already.
            return Ok(());
        }
        let quoted = shlex::try_quote(dir).context("the folder cannot be quoted")?;
        let mut output = String::new();
        let exited = self
            .exec(&format!("mkdir -p -- {quoted}"), MKDIR_LIMIT, &mut |text| {
                output.push_str(text)
            })
            .await?;
        ensure!(
            exited.exit_code == Some(0),
            "could not make {dir}: {}",
            output.trim()
        );
        Ok(())
    }

    fn same_machine(&self, other: &Self) -> bool {
        self.connection.remote_id() == other.connection.remote_id()
    }
}

/// What a copy did.
#[derive(Debug, PartialEq)]
pub(crate) struct Copied {
    /// The destination as the node resolved it.
    pub(crate) path: String,
    pub(crate) bytes: u64,
}

/// `path` on a machine: `~` is its home, a relative path starts in `base`
/// (the folder commands start in), and an absolute one stays. Without a
/// home or base, the path is left for the node, which starts relative
/// paths in its first shared folder.
pub(crate) fn on_machine(path: &str, base: Option<&str>, home: Option<&str>) -> Result<String> {
    let path = path.trim();
    ensure!(!path.is_empty(), "the path is empty");
    ensure!(path != "~" && path != "/", "{path} is a folder, not a file");
    let joined = |dir: Option<&str>, rest: &str| match dir {
        Some(dir) => format!("{}/{rest}", dir.trim_end_matches('/')),
        None => rest.to_owned(),
    };
    Ok(match path.strip_prefix("~/") {
        Some(rest) => joined(home, rest),
        None if path.starts_with('/') => path.to_owned(),
        None => joined(base.or(home), path),
    })
}

/// A path given for `picked`, resolved as `on_machine` does.
pub(crate) fn picked_path(picked: &Picked, path: &str) -> Result<String> {
    on_machine(path, picked.cwd(), picked.home.as_deref())
}

/// The file name at the end of `path`.
pub(crate) fn file_name(path: &str) -> Option<&str> {
    Path::new(path).file_name().and_then(|name| name.to_str())
}

async fn stat(node: &impl FileNode, path: &str) -> Result<Option<(String, u64, FileKind)>> {
    match node
        .files(&Request::Stat {
            path: path.to_owned(),
        })
        .await?
    {
        Reply::Stat { path, entry } => Ok(Some((path, entry.size, entry.kind))),
        Reply::Error { .. } => Ok(None),
        other => bail!("the node answered {other:?}"),
    }
}

/// Copies the file `src` on `from` to `dst` on `to`, making `dst`'s folder.
/// An existing `dst` is replaced only with `overwrite`.
pub(crate) async fn copy<N: FileNode>(
    from: &N,
    src: &str,
    to: &N,
    dst: &str,
    overwrite: bool,
) -> Result<Copied> {
    let (src, size, kind) = stat(from, src)
        .await?
        .with_context(|| format!("there is no file {src}"))?;
    ensure!(
        kind == FileKind::File,
        "{src} is not a file; to copy a folder, tar it with exec first"
    );
    ensure!(
        size <= MAX_COPY,
        "the file is larger than {} GiB",
        MAX_COPY >> 30
    );
    if let Some((existing, ..)) = stat(to, dst).await? {
        // Writing a file onto itself would empty it before it is read.
        ensure!(
            !(from.same_machine(to) && existing == src),
            "{src} is the file being copied"
        );
        ensure!(
            overwrite,
            "{dst} exists already; pass overwrite: true to replace it"
        );
    }
    if let Some(dir) = Path::new(dst).parent().and_then(Path::to_str)
        && !dir.is_empty()
    {
        to.make_dirs(dir).await?;
    }
    let mut offset = 0;
    let mut written = None;
    loop {
        let data = match from
            .files(&Request::Read {
                path: src.clone(),
                offset,
                limit: MAX_CHUNK,
            })
            .await?
        {
            Reply::Data { data, .. } => data,
            Reply::Error { message } => bail!("reading {src}: {message}"),
            other => bail!("the node answered {other:?}"),
        };
        let last = data.is_empty() || offset + data.len() as u64 >= size;
        // The first write replaces the file, even with nothing; the rest
        // append.
        if !data.is_empty() || written.is_none() {
            let length = data.len() as u64;
            match to
                .files(&Request::Write {
                    path: dst.to_owned(),
                    data,
                    append: written.is_some(),
                })
                .await?
            {
                Reply::Written { path, .. } => written = Some(path),
                Reply::Error { message } => bail!("writing {dst}: {message}"),
                other => bail!("the node answered {other:?}"),
            }
            offset += length;
            ensure!(offset <= MAX_COPY, "the file grew past the limit");
        }
        if last {
            break;
        }
    }
    Ok(Copied {
        path: written.context("nothing was written")?,
        bytes: offset,
    })
}

/// Reads all of the file `path` on `node`, up to `max` bytes.
pub(crate) async fn read_all(node: &impl FileNode, path: &str, max: u64) -> Result<Vec<u8>> {
    let (path, size, kind) = stat(node, path)
        .await?
        .with_context(|| format!("there is no file {path}"))?;
    ensure!(kind == FileKind::File, "{path} is not a file");
    ensure!(size <= max, "the file is larger than {} MiB", max >> 20);
    let mut data = Vec::new();
    loop {
        let chunk = match node
            .files(&Request::Read {
                path: path.clone(),
                offset: data.len() as u64,
                limit: MAX_CHUNK,
            })
            .await?
        {
            Reply::Data { data, .. } => data,
            Reply::Error { message } => bail!("{message}"),
            other => bail!("the node answered {other:?}"),
        };
        ensure!(
            data.len() as u64 + chunk.len() as u64 <= max,
            "the file grew past the limit"
        );
        let done = chunk.is_empty();
        data.extend_from_slice(&chunk);
        if done || data.len() as u64 >= size {
            break;
        }
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::node::files::FileEntry;
    use std::{collections::HashMap, sync::Mutex};

    /// A machine whose files are in memory. It records the largest chunk
    /// it was sent and the folders made.
    #[derive(Default)]
    struct FakeNode {
        files: Mutex<HashMap<String, Vec<u8>>>,
        dirs: Mutex<Vec<String>>,
        largest_write: Mutex<usize>,
        chunk: usize,
    }

    impl FakeNode {
        fn with(chunk: usize, files: &[(&str, Vec<u8>)]) -> Self {
            Self {
                files: Mutex::new(
                    files
                        .iter()
                        .map(|(path, data)| ((*path).to_owned(), data.clone()))
                        .collect(),
                ),
                chunk,
                ..Self::default()
            }
        }

        fn file(&self, path: &str) -> Option<Vec<u8>> {
            self.files.lock().unwrap().get(path).cloned()
        }
    }

    #[async_trait]
    impl FileNode for FakeNode {
        async fn files(&self, request: &Request) -> Result<Reply> {
            let mut files = self.files.lock().unwrap();
            Ok(match request {
                Request::Stat { path } => match files.get(path) {
                    Some(data) => Reply::Stat {
                        path: path.clone(),
                        entry: FileEntry {
                            name: path.clone(),
                            kind: FileKind::File,
                            size: data.len() as u64,
                            modified: None,
                        },
                    },
                    None => Reply::Error {
                        message: "no such file".into(),
                    },
                },
                Request::Read {
                    path,
                    offset,
                    limit,
                } => {
                    let data = files.get(path).cloned().unwrap_or_default();
                    let start = (*offset as usize).min(data.len());
                    let end = (start + self.chunk.min(*limit as usize)).min(data.len());
                    Reply::Data {
                        path: path.clone(),
                        data: data[start..end].to_vec(),
                        size: data.len() as u64,
                    }
                }
                Request::Write { path, data, append } => {
                    let mut largest = self.largest_write.lock().unwrap();
                    *largest = (*largest).max(data.len());
                    let file = files.entry(path.clone()).or_default();
                    if !*append {
                        file.clear();
                    }
                    file.extend_from_slice(data);
                    Reply::Written {
                        path: path.clone(),
                        size: file.len() as u64,
                    }
                }
                other => Reply::Error {
                    message: format!("unexpected {other:?}"),
                },
            })
        }

        async fn make_dirs(&self, dir: &str) -> Result<()> {
            self.dirs.lock().unwrap().push(dir.to_owned());
            Ok(())
        }

        fn same_machine(&self, other: &Self) -> bool {
            std::ptr::eq(self, other)
        }
    }

    #[tokio::test]
    async fn a_file_is_not_copied_onto_itself() {
        let node = FakeNode::with(4096, &[("/a", b"keep".to_vec())]);
        let refused = copy(&node, "/a", &node, "/a", true).await.unwrap_err();
        assert!(refused.to_string().contains("being copied"), "{refused}");
        assert_eq!(node.file("/a"), Some(b"keep".to_vec()));
    }

    #[tokio::test]
    async fn a_copy_goes_chunk_by_chunk_and_makes_the_folder() {
        let data: Vec<u8> = (0..10_000u32).map(|n| (n % 251) as u8).collect();
        let from = FakeNode::with(4096, &[("/home/user/big.bin", data.clone())]);
        let to = FakeNode::with(4096, &[]);
        let copied = copy(
            &from,
            "/home/user/big.bin",
            &to,
            "/Users/me/in/big.bin",
            false,
        )
        .await
        .unwrap();
        assert_eq!(
            copied,
            Copied {
                path: "/Users/me/in/big.bin".into(),
                bytes: 10_000
            }
        );
        assert_eq!(to.file("/Users/me/in/big.bin"), Some(data));
        assert_eq!(*to.largest_write.lock().unwrap(), 4096);
        assert_eq!(*to.dirs.lock().unwrap(), ["/Users/me/in"]);
    }

    #[tokio::test]
    async fn a_copy_does_not_replace_a_file_unless_asked() {
        let from = FakeNode::with(4096, &[("/a", b"new".to_vec())]);
        let to = FakeNode::with(4096, &[("/b", b"old".to_vec())]);
        let refused = copy(&from, "/a", &to, "/b", false).await.unwrap_err();
        assert!(refused.to_string().contains("overwrite"), "{refused}");
        assert_eq!(to.file("/b"), Some(b"old".to_vec()));
        copy(&from, "/a", &to, "/b", true).await.unwrap();
        assert_eq!(to.file("/b"), Some(b"new".to_vec()));
    }

    #[tokio::test]
    async fn an_empty_file_and_a_missing_one() {
        let from = FakeNode::with(4096, &[("/empty", Vec::new())]);
        let to = FakeNode::with(4096, &[]);
        let copied = copy(&from, "/empty", &to, "/copy", false).await.unwrap();
        assert_eq!(copied.bytes, 0);
        assert_eq!(to.file("/copy"), Some(Vec::new()));
        assert!(copy(&from, "/nothing", &to, "/x", false).await.is_err());
    }

    #[tokio::test]
    async fn reading_stops_at_the_limit() {
        let node = FakeNode::with(3, &[("/f", b"abcdefgh".to_vec())]);
        assert_eq!(read_all(&node, "/f", 100).await.unwrap(), b"abcdefgh");
        assert!(read_all(&node, "/f", 4).await.is_err());
    }

    #[test]
    fn paths_resolve_on_each_machine() {
        let home = Some("/Users/me");
        assert_eq!(
            on_machine("~/a.pdf", None, home).unwrap(),
            "/Users/me/a.pdf"
        );
        assert_eq!(
            on_machine("a.pdf", Some("/home/user"), home).unwrap(),
            "/home/user/a.pdf"
        );
        assert_eq!(
            on_machine("x/a.pdf", None, home).unwrap(),
            "/Users/me/x/a.pdf"
        );
        assert_eq!(on_machine("/srv/a.pdf", None, home).unwrap(), "/srv/a.pdf");
        assert_eq!(on_machine("a.pdf", None, None).unwrap(), "a.pdf");
        assert!(on_machine("~", None, home).is_err());
        assert!(on_machine(" ", None, home).is_err());
        assert_eq!(file_name("/home/user/r.pdf"), Some("r.pdf"));
    }
}
