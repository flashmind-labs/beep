//! A coding agent session's working folder on its machine, for the apps'
//! Files tab: listing it, reading any file in it and writing files dropped
//! on the session. The node checks every path stays inside the folder;
//! the server passes the parts through and keeps nothing.

use super::{Reply, agent_control::live_host};
use crate::{
    nodes::{NodeFile, NodeWork, WorkRead, WorkWrite},
    protocol::{
        AGENT_FILE_CHUNK, AgentFileChunk, ApiError, ListWorkFolder, ListWorkTree, MAX_WORK_UPLOAD,
        ReadWorkFile, WorkFolder, WorkTree, WriteWorkFile,
    },
    router::Cx,
};

/// What the caller sees when the node could not be asked: one from before
/// these requests closes the stream.
fn unreachable_node(machine: &str, e: &anyhow::Error) -> ApiError {
    ApiError::bad_request(format!(
        "Could not reach the session's folder on {machine}: {e:#}. Updating beep-node there may help."
    ))
}

fn refused(machine: &str, message: &str) -> ApiError {
    ApiError::bad_request(format!("{machine}: {message}"))
}

pub(super) async fn list_work_folder(cx: Cx, list: ListWorkFolder) -> Reply<WorkFolder> {
    let live = live_host(&cx, &list.machine_id).await?;
    let machine = &live.host.machine_name;
    match live
        .online
        .work_folder(&list.root, &list.path, list.ignored)
        .await
        .map_err(|e| unreachable_node(machine, &e))?
    {
        NodeWork::Done(folder) => Ok(folder),
        NodeWork::Refused(message) => Err(refused(machine, &message)),
    }
}

pub(super) async fn list_work_tree(cx: Cx, list: ListWorkTree) -> Reply<WorkTree> {
    let live = live_host(&cx, &list.machine_id).await?;
    let machine = &live.host.machine_name;
    match live
        .online
        .work_tree(&list)
        .await
        .map_err(|e| unreachable_node(machine, &e))?
    {
        NodeWork::Done(tree) => Ok(tree),
        NodeWork::Refused(message) => Err(refused(machine, &message)),
    }
}

pub(super) async fn read_work_file(cx: Cx, read: ReadWorkFile) -> Reply<AgentFileChunk> {
    let live = live_host(&cx, &read.machine_id).await?;
    let machine = &live.host.machine_name;
    let len = read
        .len
        .min(u32::try_from(AGENT_FILE_CHUNK).unwrap_or(u32::MAX));
    let part = WorkRead {
        root: &read.root,
        path: &read.path,
        offset: read.offset,
        len,
    };
    match live
        .online
        .read_work_file(part)
        .await
        .map_err(|e| unreachable_node(machine, &e))?
    {
        NodeFile::Chunk { size, mime, data } => Ok(AgentFileChunk { size, mime, data }),
        NodeFile::Missing => Err(ApiError::NotFound {}),
        NodeFile::Refused(message) => Err(refused(machine, &message)),
    }
}

pub(super) async fn write_work_file(cx: Cx, write: WriteWorkFile) -> Reply<String> {
    if write.size > MAX_WORK_UPLOAD {
        return Err(ApiError::bad_request(format!(
            "Files up to {} MB can be dropped on a session.",
            MAX_WORK_UPLOAD / (1024 * 1024)
        )));
    }
    if write.data.len() > AGENT_FILE_CHUNK {
        return Err(ApiError::bad_request("A part of the file is too large."));
    }
    let live = live_host(&cx, &write.machine_id).await?;
    let machine = &live.host.machine_name;
    let part = WorkWrite {
        root: write.root,
        path: write.path,
        offset: write.offset,
        size: write.size,
        data: write.data,
    };
    match live
        .online
        .write_work_file(part)
        .await
        .map_err(|e| unreachable_node(machine, &e))?
    {
        NodeWork::Done(path) => Ok(path),
        NodeWork::Refused(message) => Err(refused(machine, &message)),
    }
}
