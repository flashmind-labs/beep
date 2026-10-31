//! The user's environment: a linked node started with `--workspace`, such
//! as the Ubuntu container deploy/onprem bundles. `exec` runs there unless
//! told otherwise, and files apps and the user hand over are saved to its
//! workspace folder (`/home/user` in the bundle) over its `files/1`.
//!
//! Paths the model sees and gives are relative to the workspace, or
//! absolute under it, the same as for commands, which start there.

use crate::{
    app::App,
    fetch::{self, MAX_DOWNLOAD},
    nodes::Online,
    protocol::node::files::{FileKind, MAX_CHUNK, Reply, Request},
    user::User,
};
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::{
    path::{Component, Path},
    sync::Arc,
};

/// The user's environment, looked up again on each use so a node that
/// reconnects mid-turn is still found.
pub struct Workspace {
    app: Arc<App>,
    user: String,
}

/// A node that is the user's environment, with its folder.
struct Linked {
    node: Online,
    root: String,
}

#[derive(Serialize)]
pub(crate) struct Saved {
    pub(crate) path: String,
    bytes: u64,
    content_type: Option<String>,
}

/// A file read from the workspace.
pub(crate) struct FolderFile {
    pub(crate) name: String,
    pub(crate) data: Vec<u8>,
}

impl Workspace {
    /// The user's environment, when one is online now.
    pub(crate) fn find(app: &Arc<App>, user: &User) -> Option<Arc<Self>> {
        app.nodes.workspace(user.id())?;
        Some(Arc::new(Self {
            app: app.clone(),
            user: user.id().to_owned(),
        }))
    }

    fn linked(&self) -> Result<Linked> {
        let node = self
            .app
            .nodes
            .workspace(&self.user)
            .context("the user's environment is offline")?;
        let root = node
            .workspace
            .clone()
            .context("the node has no workspace")?;
        Ok(Linked { node, root })
    }

    /// Fetches a public `url` and saves it as `name` (or the URL's name).
    pub(crate) async fn download(&self, url: &str, name: Option<&str>) -> Result<Saved> {
        let fetched = fetch::fetch_public(url).await?;
        let name = name
            .filter(|name| !name.trim().is_empty())
            .map(fetch::clean_file_name)
            .unwrap_or(fetched.name);
        self.write(&name, &fetched.data, fetched.content_type).await
    }

    /// Saves `data` as `name` in the workspace, for a file an app or the user
    /// hands over itself rather than as a link.
    pub(crate) async fn save(&self, name: &str, data: &[u8]) -> Result<Saved> {
        self.write(&fetch::clean_file_name(name), data, None).await
    }

    async fn write(&self, name: &str, data: &[u8], content_type: Option<String>) -> Result<Saved> {
        ensure!(
            data.len() as u64 <= MAX_DOWNLOAD,
            "the file is larger than {} MiB",
            MAX_DOWNLOAD >> 20
        );
        let Linked { node, root } = self.linked()?;
        let path = format!("{root}/{name}");
        let mut written = None;
        // The first write replaces the file, even with nothing; the rest
        // append.
        let mut chunks: Vec<&[u8]> = data.chunks(MAX_CHUNK as usize).collect();
        if chunks.is_empty() {
            chunks.push(&[]);
        }
        for (n, chunk) in chunks.into_iter().enumerate() {
            let request = Request::Write {
                path: path.clone(),
                data: chunk.to_vec(),
                append: n > 0,
            };
            match node.files(&request).await? {
                Reply::Written { path, .. } => written = Some(path),
                Reply::Error { message } => bail!("{message}"),
                other => bail!("the node answered {other:?}"),
            }
        }
        let written = written.context("nothing was written")?;
        Ok(Saved {
            path: shown(&root, &written),
            bytes: data.len() as u64,
            content_type,
        })
    }

    /// The file at `path`, relative to the workspace or absolute under it.
    pub(crate) async fn read(&self, path: &str) -> Result<FolderFile> {
        let Linked { node, root } = self.linked()?;
        let path = in_workspace(&root, path)?;
        let entry = match node.files(&Request::Stat { path: path.clone() }).await? {
            Reply::Stat { entry, .. } => entry,
            Reply::Error { message } => bail!("{message}"),
            other => bail!("the node answered {other:?}"),
        };
        ensure!(entry.kind == FileKind::File, "it is not a file");
        ensure!(
            entry.size <= MAX_DOWNLOAD,
            "the file is larger than {} MiB",
            MAX_DOWNLOAD >> 20
        );
        let mut data = Vec::new();
        loop {
            let request = Request::Read {
                path: path.clone(),
                offset: data.len() as u64,
                limit: MAX_CHUNK,
            };
            let (chunk, size) = match node.files(&request).await? {
                Reply::Data { data, size, .. } => (data, size),
                Reply::Error { message } => bail!("{message}"),
                other => bail!("the node answered {other:?}"),
            };
            ensure!(size <= MAX_DOWNLOAD, "the file grew past the limit");
            let done = chunk.is_empty();
            data.extend_from_slice(&chunk);
            if done || data.len() as u64 >= size {
                break;
            }
        }
        let name = Path::new(&path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file")
            .to_owned();
        Ok(FolderFile { name, data })
    }
}

/// `path` as an absolute path under `root`: given relative to it, as
/// `~/...`, or absolute under it. Anything that could lead out is refused;
/// the node checks links as well.
fn in_workspace(root: &str, path: &str) -> Result<String> {
    let path = path.trim();
    let outside = || format!("the path must be inside {root}");
    let relative = match path.strip_prefix(root) {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => rest.trim_start_matches('/'),
        _ if path.starts_with('/') => bail!(outside()),
        _ if path == "~" => "",
        _ => path.strip_prefix("~/").unwrap_or(path),
    };
    let parts = Path::new(relative).components();
    ensure!(
        parts
            .clone()
            .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
            && parts
                .clone()
                .any(|part| matches!(part, Component::Normal(_))),
        outside()
    );
    let clean: Vec<_> = parts
        .filter_map(|part| match part {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect();
    Ok(format!("{root}/{}", clean.join("/")))
}

/// `path` as the model sees it: relative to `root` when inside it.
fn shown(root: &str, path: &str) -> String {
    path.strip_prefix(root)
        .and_then(|rest| rest.strip_prefix('/'))
        .filter(|rest| !rest.is_empty())
        .unwrap_or(path)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_map_into_the_workspace() {
        for (given, absolute) in [
            ("report.pdf", "/home/user/report.pdf"),
            ("./report.pdf", "/home/user/report.pdf"),
            ("~/charts/a.png", "/home/user/charts/a.png"),
            ("/home/user/report.pdf", "/home/user/report.pdf"),
            (" /home/user//charts/./a.png ", "/home/user/charts/a.png"),
        ] {
            assert_eq!(
                in_workspace("/home/user", given).expect("inside"),
                absolute,
                "{given}"
            );
        }
        for given in [
            "",
            "/home/user",
            "/home/user/",
            "/home/username/a.pdf",
            "/home/user/../beep/a.pdf",
            "/etc/hosts",
            "../a.pdf",
            "a/../../b",
            "~",
        ] {
            assert!(in_workspace("/home/user", given).is_err(), "{given:?}");
        }
    }

    #[test]
    fn saved_paths_show_relative_to_the_workspace() {
        assert_eq!(shown("/home/user", "/home/user/a.pdf"), "a.pdf");
        assert_eq!(shown("/home/user", "/home/user/x/a.pdf"), "x/a.pdf");
        assert_eq!(shown("/home/user", "/home/username/a"), "/home/username/a");
        assert_eq!(shown("/home/user", "/srv/a"), "/srv/a");
    }
}
