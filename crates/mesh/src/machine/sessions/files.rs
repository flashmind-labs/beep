use super::{
    IMAGE_TIMEOUT, NodeFile, NodeShare, NodeWork, Picture, READ_TIMEOUT, Reading, SHARE_TIMEOUT,
    Sessions, WorkRead, WorkWrite, ask, hinted, once,
};
use crate::{MeshError, machine::unexpected};
use beep_proto::SessionId;
use beep_proto::{
    CustomAgent, ListWorkTree, WorkFolder, WorkTree,
    node::agent::{LspDefinitions, RelPath, SessionsReply, SessionsRequest},
};
use std::time::Duration;

/// The picture `key` of `session`'s transcript; `None` when the node no longer has it.
pub async fn image<S: Sessions + ?Sized>(
    node: &S,
    session: &SessionId,
    key: &str,
) -> Result<Option<Picture>, MeshError> {
    let request = SessionsRequest::Image {
        session: session.clone(),
        key: key.to_owned(),
    };
    picture(node, request).await
}

/// `image` made small on the node, so only the thumbnail crosses the
/// network. A node from before thumbnails closes the stream: the caller
/// then asks for the whole picture.
pub async fn thumb<S: Sessions + ?Sized>(
    node: &S,
    session: &SessionId,
    key: &str,
) -> Result<Option<Picture>, MeshError> {
    let request = SessionsRequest::Thumb {
        session: session.clone(),
        key: key.to_owned(),
    };
    picture(node, request).await
}

async fn picture<S: Sessions + ?Sized>(
    node: &S,
    request: SessionsRequest,
) -> Result<Option<Picture>, MeshError> {
    match node.sessions(request, Reading::Once, IMAGE_TIMEOUT).await? {
        SessionsReply::Image { mime, data } => Ok(Some(Picture { mime, data })),
        SessionsReply::Error { .. } => Ok(None),
        other => Err(unexpected(&other)),
    }
}

/// Part of the media file `path` on the node, from `offset`.
pub async fn media<S: Sessions + ?Sized>(
    node: &S,
    path: &str,
    offset: u64,
) -> Result<NodeFile, MeshError> {
    let request = SessionsRequest::File {
        path: path.to_owned(),
        offset,
    };
    file_reply(once(node, request).await?)
}

/// Lets this device fetch the media file `path` on the node over
/// iroh-blobs for a few minutes. The node hashes it first, which for a
/// large file takes longer than an ordinary read.
pub async fn share<S: Sessions + ?Sized>(node: &S, path: &str) -> Result<NodeShare, MeshError> {
    let request = SessionsRequest::Share {
        path: path.to_owned(),
    };

    match node.sessions(request, Reading::Once, SHARE_TIMEOUT).await? {
        SessionsReply::Shared { hash, size } => Ok(NodeShare::Shared { hash, size }),
        SessionsReply::NoFile {} => Ok(NodeShare::Missing),
        SessionsReply::Error { message } => Ok(NodeShare::Refused(message)),
        other => Err(unexpected(&other)),
    }
}

/// Part of a file inside a session folder on the node.
pub async fn read_file<S: Sessions + ?Sized>(
    node: &S,
    read: WorkRead<'_>,
) -> Result<NodeFile, MeshError> {
    let request = SessionsRequest::ReadFile {
        root: read.root.to_owned(),
        path: read.path.to_owned(),
        offset: read.offset,
        len: read.len,
    };
    file_reply(once(node, request).await?)
}

fn file_reply(reply: SessionsReply) -> Result<NodeFile, MeshError> {
    match reply {
        SessionsReply::File { size, mime, data } => Ok(NodeFile::Chunk { size, mime, data }),
        SessionsReply::NoFile {} => Ok(NodeFile::Missing),
        SessionsReply::Error { message } => Ok(NodeFile::Refused(message)),
        other => Err(unexpected(&other)),
    }
}

/// Whether each of `paths` is on the node, in order; `None` from a node
/// from before the question.
pub async fn present<S: Sessions + ?Sized>(
    node: &S,
    paths: Vec<String>,
) -> Result<Option<Vec<bool>>, MeshError> {
    match once(node, SessionsRequest::Present { paths }).await? {
        SessionsReply::Present { present } => Ok(Some(present)),
        SessionsReply::Error { .. } => Ok(None),
        other => Err(unexpected(&other)),
    }
}

/// The BLAKE3 hash of each of `paths` inside the session folder `root` on
/// the node, in order, `None` for one it does not have; `None` in all from a
/// node from before the question, or one that refused it.
pub async fn hashes<S: Sessions + ?Sized>(
    node: &S,
    root: &str,
    paths: Vec<String>,
) -> Result<Option<Vec<Option<[u8; 32]>>>, MeshError> {
    let request = SessionsRequest::HashFiles {
        root: root.to_owned(),
        paths,
    };

    match once(node, request).await? {
        SessionsReply::Hashes { hashes } => Ok(Some(
            hashes
                .into_iter()
                .map(|hash| hash.map(serde_bytes::ByteArray::into_array))
                .collect(),
        )),
        SessionsReply::Error { .. } => Ok(None),
        other => Err(unexpected(&other)),
    }
}

/// The folder `path` inside the session folder `root` on the node.
pub async fn folder<S: Sessions + ?Sized>(
    node: &S,
    root: &str,
    path: &str,
    ignored: bool,
) -> Result<NodeWork<WorkFolder>, MeshError> {
    let request = SessionsRequest::Folder {
        root: root.to_owned(),
        path: path.to_owned(),
        ignored,
        depth: 0,
    };

    match ask(node, request).await? {
        SessionsReply::Folder { folder } => Ok(NodeWork::Done(folder)),
        SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
        other => Err(unexpected(&other)),
    }
}

/// The folder `ask.path` inside `ask.root` and those under it, `ask.depth`
/// levels in all; `ask.path` alone from a node from before trees.
pub async fn tree<S: Sessions + ?Sized>(
    node: &S,
    list: &ListWorkTree,
    reading: Reading,
) -> Result<NodeWork<WorkTree>, MeshError> {
    let request = SessionsRequest::Folder {
        root: list.root.clone(),
        path: list.path.clone(),
        ignored: list.ignored,
        depth: list.depth.max(1),
    };

    match node.sessions(request, reading, READ_TIMEOUT).await? {
        SessionsReply::Tree { tree, .. } => Ok(NodeWork::Done(tree)),
        SessionsReply::Folder { folder } => Ok(NodeWork::Done(WorkTree {
            folders: vec![folder],
            truncated: false,
        })),
        SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
        other => Err(unexpected(&other)),
    }
}

/// Writes one part of a file into a session folder on the node; gives the path it went to.
pub async fn write_file<S: Sessions + ?Sized>(
    node: &S,
    write: WorkWrite,
) -> Result<NodeWork<String>, MeshError> {
    let request = SessionsRequest::WriteFile {
        root: write.root,
        path: write.path,
        offset: write.offset,
        size: write.size,
        data: write.data,
    };

    match once(node, request).await? {
        SessionsReply::Written { path } => Ok(NodeWork::Done(path)),
        SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
        other => Err(unexpected(&other)),
    }
}

/// Writes a session attachment into the host cache. The returned path is absolute.
pub async fn upload_file<S: Sessions + ?Sized>(
    node: &S,
    agent: String,
    session: SessionId,
    write: WorkWrite,
) -> Result<NodeWork<String>, MeshError> {
    let request = SessionsRequest::UploadFile {
        root: write.root,
        agent,
        session,
        path: write.path,
        offset: write.offset,
        size: write.size,
        data: write.data,
    };
    let reply = once(node, request).await?;

    match reply {
        SessionsReply::Written { path } => Ok(NodeWork::Done(path)),
        SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
        other => Err(unexpected(&other)),
    }
}

/// Edits files once. Mutations are never retried after a lost reply.
pub async fn edit_files<S: Sessions + ?Sized>(
    node: &S,
    root: String,
    action: beep_proto::node::agent::FileAction,
) -> Result<NodeWork<()>, MeshError> {
    let reply = hinted(
        node.sessions(
            SessionsRequest::EditFiles { root, action },
            Reading::Once,
            Duration::from_secs(300),
        )
        .await,
        "to edit files",
    )?;

    match reply {
        SessionsReply::FilesEdited {} => Ok(NodeWork::Done(())),
        SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
        other => Err(unexpected(&other)),
    }
}

/// Removes an upload that was taken off a draft.
pub async fn discard_draft_attachment<S: Sessions + ?Sized>(
    node: &S,
    draft: SessionId,
    path: String,
) -> Result<NodeWork<()>, MeshError> {
    let request = SessionsRequest::DiscardDraftAttachment { draft, path };

    match once(node, request).await? {
        SessionsReply::FilesEdited {} => Ok(NodeWork::Done(())),
        SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
        other => Err(unexpected(&other)),
    }
}

/// The custom agents the harnesses define for `folder` on the node and in its home.
pub async fn custom_agents<S: Sessions + ?Sized>(
    node: &S,
    folder: &str,
) -> Result<NodeWork<Vec<CustomAgent>>, MeshError> {
    let request = SessionsRequest::CustomAgents {
        folder: folder.to_owned(),
    };

    match once(node, request).await? {
        SessionsReply::CustomAgents { agents } => Ok(NodeWork::Done(agents)),
        SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
        other => Err(unexpected(&other)),
    }
}

/// How long an LSP definition lookup may wait, including cold server startup.
const LSP_TIMEOUT: Duration = Duration::from_secs(35);

/// Looks up the definition at 0-based `line` and UTF-16 `column` of `path`
/// inside `root` on the node.
pub async fn lsp_definition<S: Sessions + ?Sized>(
    node: &S,
    root: String,
    path: RelPath,
    line: u32,
    column: u32,
    text: Option<String>,
) -> Result<NodeWork<LspDefinitions>, MeshError> {
    let request = SessionsRequest::LspDefinition {
        root,
        path,
        line,
        column,
        text,
    };
    let reply = hinted(
        node.sessions(request, Reading::Once, LSP_TIMEOUT).await,
        "look up code definitions",
    )?;

    match reply {
        SessionsReply::LspLocations { result } => Ok(NodeWork::Done(result)),
        SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
        other => Err(unexpected(&other)),
    }
}
