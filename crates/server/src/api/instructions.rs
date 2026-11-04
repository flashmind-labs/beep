//! The user's instructions for every agent, kept between markers in agent files on their machines.

use super::{Reply, own_nodes::linked_keys};
use crate::{
    api::router::Cx,
    nodes::Online,
    protocol::*,
    store::{InstructionsRow, Saved},
};
use beep_mesh::{
    PeerClient,
    machine::agent_files::{self, ReadFile},
};
use futures::{StreamExt, stream};

/// The longest instructions, in bytes.
const MAX_TEXT: usize = 16 * 1024;
const MAX_NOTE: usize = 200;
/// Machines read at once.
const IN_FLIGHT: usize = 4;

pub(super) async fn get_instructions(
    cx: Cx,
    _: GetInstructions,
) -> Reply<Option<InstructionsVersion>> {
    Ok(newest(&cx).await?.map(view))
}

pub(super) async fn save_instructions(
    cx: Cx,
    request: SaveInstructions,
) -> Reply<InstructionsVersion> {
    if request.text.len() > MAX_TEXT {
        return Err(ApiError::bad_request(
            "Instructions can be at most 16 KB.".to_owned(),
        ));
    }
    let note = request.note.trim();
    if note.len() > MAX_NOTE || note.chars().any(char::is_control) {
        return Err(ApiError::bad_request(format!(
            "A note can be at most {MAX_NOTE} characters on one line."
        )));
    }
    match cx
        .user
        .save_instructions(request.base, &request.text, note)
        .await?
    {
        Saved::Made(row) => Ok(view(row)),
        Saved::Stale(newest) => Err(ApiError::Conflict {
            message: format!("Version {newest} was saved since. Load it, then edit again."),
        }),
    }
}

pub(super) async fn list_instructions_versions(
    cx: Cx,
    _: ListInstructionsVersions,
) -> Reply<Vec<InstructionsVersion>> {
    Ok(cx
        .user
        .instructions_versions(50)
        .await?
        .into_iter()
        .map(view)
        .collect())
}

pub(super) async fn read_agent_files(cx: Cx, _: ReadAgentFiles) -> Reply<Vec<MachineAgentFiles>> {
    let text = newest(&cx).await?.map(|row| row.text);
    let nodes = machines(&cx).await?;
    let mut read: Vec<MachineAgentFiles> = stream::iter(nodes)
        .map(|node| {
            let text = text.clone();
            async move {
                let files = agent_files::read(&asking(&node)).await;
                machine_files(&node, files, text.as_deref())
            }
        })
        .buffer_unordered(IN_FLIGHT)
        .collect()
        .await;
    read.sort_by(|a, b| a.machine_name.cmp(&b.machine_name));
    Ok(read)
}

pub(super) async fn sync_agent_files(cx: Cx, request: SyncAgentFiles) -> Reply<MachineAgentFiles> {
    let Some(newest) = newest(&cx).await? else {
        return Err(ApiError::bad_request(
            "Save your instructions before writing them to agent files.".to_owned(),
        ));
    };
    let node = machines(&cx)
        .await?
        .into_iter()
        .find(|node| node.connection.remote_id().to_string() == request.machine_id)
        .ok_or(ApiError::NotFound {})?;
    let fail = |e: beep_mesh::MeshError| ApiError::bad_request(format!("{}: {e:#}", node.name));
    let client = asking(&node);
    let before = agent_files::read(&client).await.map_err(fail)?;
    agent_files::write(&client, &before, &newest.text)
        .await
        .map_err(fail)?;
    let after = agent_files::read(&client).await;
    Ok(machine_files(&node, after, Some(&newest.text)))
}

fn asking(node: &Online) -> PeerClient {
    PeerClient::new(node.connection.clone())
}

/// How `node`'s agent files stand, for the apps.
fn machine_files(
    node: &Online,
    files: Result<Vec<ReadFile>, beep_mesh::MeshError>,
    newest: Option<&str>,
) -> MachineAgentFiles {
    agent_files::machine_files(
        node.connection.remote_id().to_string(),
        node.name.clone(),
        files,
        newest,
    )
}

async fn newest(cx: &Cx) -> Result<Option<InstructionsRow>, ApiError> {
    Ok(cx.user.instructions_versions(1).await?.into_iter().next())
}

fn view(row: InstructionsRow) -> InstructionsVersion {
    InstructionsVersion {
        version: row.version,
        text: row.text,
        note: row.note,
        saved_at: row.saved_at.into(),
    }
}

/// The caller's online machines whose keys are still linked.
async fn machines(cx: &Cx) -> Result<Vec<Online>, ApiError> {
    let keys = linked_keys(cx).await?;
    Ok(cx
        .app
        .nodes
        .of_user(cx.user.id())
        .into_iter()
        .filter(|node| keys.contains(&node.connection.remote_id().to_string()))
        .collect())
}
