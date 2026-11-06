//! A session's working folder for the Files tab: a desktop reads it over its mesh now.

use super::{Reply, own_nodes::retired};
use crate::{
    api::router::Cx,
    protocol::{
        AgentFileChunk, ListWorkFolder, ListWorkTree, ReadWorkFile, WorkFolder, WorkTree,
        WriteWorkFile,
    },
};

pub(super) async fn list_work_folder(_: Cx, _: ListWorkFolder) -> Reply<WorkFolder> {
    retired()
}

pub(super) async fn list_work_tree(_: Cx, _: ListWorkTree) -> Reply<WorkTree> {
    retired()
}

pub(super) async fn read_work_file(_: Cx, _: ReadWorkFile) -> Reply<AgentFileChunk> {
    retired()
}

pub(super) async fn write_work_file(_: Cx, _: WriteWorkFile) -> Reply<String> {
    retired()
}
