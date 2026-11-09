use super::{
    IMAGE_TIMEOUT, NodeFile, NodeImage, NodeWork, Sessions, WorkRead, WorkWrite, ask, hinted,
};
use crate::{MeshError, machine::unexpected};
use beep_proto::{
    CustomAgent, ListWorkTree, WorkFolder, WorkTree,
    node::agent::{SessionsReply, SessionsRequest},
};
use std::time::Duration;

/// The picture `key` of `session`'s transcript; `None` when the node no longer has it.
pub async fn image<S: Sessions + ?Sized>(
    node: &S,
    session: &str,
    key: &str,
) -> Result<Option<NodeImage>, MeshError> {
    let request = SessionsRequest::Image {
        session: session.to_owned(),
        key: key.to_owned(),
    };
    picture(node, request).await
}

/// `image` made small on the node, so only the thumbnail crosses the
/// network. A node from before thumbnails closes the stream: the caller
/// then asks for the whole picture.
pub async fn thumb<S: Sessions + ?Sized>(
    node: &S,
    session: &str,
    key: &str,
) -> Result<Option<NodeImage>, MeshError> {
    let request = SessionsRequest::Thumb {
        session: session.to_owned(),
        key: key.to_owned(),
    };
    picture(node, request).await
}

async fn picture<S: Sessions + ?Sized>(
    node: &S,
    request: SessionsRequest,
) -> Result<Option<NodeImage>, MeshError> {
    match node.sessions(request, IMAGE_TIMEOUT).await? {
        SessionsReply::Image { mime, data } => Ok(Some(NodeImage { mime, data })),
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
    file_reply(ask(node, request).await?)
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
    file_reply(ask(node, request).await?)
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
    match ask(node, SessionsRequest::Present { paths }).await? {
        SessionsReply::Present { present } => Ok(Some(present)),
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
) -> Result<NodeWork<WorkTree>, MeshError> {
    let request = SessionsRequest::Folder {
        root: list.root.clone(),
        path: list.path.clone(),
        ignored: list.ignored,
        depth: list.depth.max(1),
    };

    match ask(node, request).await? {
        SessionsReply::Tree { tree } => Ok(NodeWork::Done(tree)),
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

    match ask(node, request).await? {
        SessionsReply::Written { path } => Ok(NodeWork::Done(path)),
        SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
        other => Err(unexpected(&other)),
    }
}

/// Writes a session attachment into the host cache. The returned path is absolute.
pub async fn upload_file<S: Sessions + ?Sized>(
    node: &S,
    agent: String,
    session: String,
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
    let reply = hinted(ask(node, request).await, "upload temporary attachments")?;

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

/// The custom agents the harnesses define for `folder` on the node and in its home.
pub async fn custom_agents<S: Sessions + ?Sized>(
    node: &S,
    folder: &str,
) -> Result<NodeWork<Vec<CustomAgent>>, MeshError> {
    let request = SessionsRequest::CustomAgents {
        folder: folder.to_owned(),
    };

    match ask(node, request).await? {
        SessionsReply::CustomAgents { agents } => Ok(NodeWork::Done(agents)),
        SessionsReply::Error { message } => Ok(NodeWork::Refused(message)),
        other => Err(unexpected(&other)),
    }
}
