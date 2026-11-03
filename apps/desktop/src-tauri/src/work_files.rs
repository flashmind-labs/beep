//! The Files tab of a coding agent session: its working folder on the
//! session's machine listed a few levels at a time, a file read to show as
//! text or saved to Downloads, and files dropped on the session uploaded
//! into the folder a part at a time.

use crate::{
    beep::Beep,
    commands::{GONE, file_name, free_name},
    error::{DesktopError, Result},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::{
    AGENT_FILE_CHUNK, AgentFileChunk, ApiError, GitChange, ListWorkTree, MAX_AGENT_FILE,
    ReadWorkFile, WriteWorkFile,
};
use serde::Serialize;
use std::{fs, path::PathBuf, sync::Arc};
use tauri::{AppHandle, Manager, State};

type Service<'a> = State<'a, Arc<Beep>>;

/// The most of a file the text view shows.
const TEXT_CAP: u32 = 512 * 1024;

/// One folder of the working folder, for the bar.
#[derive(Debug, Serialize)]
pub struct TreeFolder {
    path: String,
    entries: Vec<TreeEntry>,
    more: u32,
    git: bool,
}

#[derive(Debug, Serialize)]
pub struct TreeEntry {
    name: String,
    folder: bool,
    size: u64,
    ignored: bool,
    /// `untracked`, `added`, `modified` or `conflicted`.
    change: Option<&'static str>,
}

fn change_name(change: GitChange) -> &'static str {
    match change {
        GitChange::Untracked {} => "untracked",
        GitChange::Added {} => "added",
        GitChange::Modified {} => "modified",
        GitChange::Conflicted {} => "conflicted",
    }
}

/// The start of a file, for the text view: `text` is its first bytes as
/// UTF-8, `cut` when there is more, `binary` when it does not read as text.
#[derive(Debug, Serialize)]
pub struct TreeText {
    size: u64,
    text: String,
    cut: bool,
    binary: bool,
}

/// One `ReadWorkFile`, with a file the machine no longer has as `GONE`.
pub(crate) async fn read_part(beep: &Beep, read: ReadWorkFile) -> Result<AgentFileChunk> {
    match beep.call(read).await {
        Err(DesktopError::Api(ApiError::NotFound {})) => Err(DesktopError::BadRequest(GONE.into())),
        other => other,
    }
}

/// The folders `TREE_DEPTH` levels down from `path`, for the bar.
#[derive(Debug, Serialize)]
pub struct Tree {
    folders: Vec<TreeFolder>,
    truncated: bool,
}

/// How many levels one read of the tree takes.
const TREE_DEPTH: u32 = 3;

#[tauri::command]
pub async fn agent_tree(
    beep: Service<'_>,
    machine_id: String,
    root: String,
    path: String,
    ignored: bool,
) -> Result<Tree> {
    let tree = beep
        .call(ListWorkTree {
            machine_id,
            root,
            path,
            ignored,
            depth: TREE_DEPTH,
        })
        .await?;
    let folders = tree
        .folders
        .into_iter()
        .map(|folder| TreeFolder {
            path: folder.path,
            entries: folder
                .entries
                .into_iter()
                .map(|e| TreeEntry {
                    name: e.name,
                    folder: e.folder,
                    size: e.size,
                    ignored: e.ignored,
                    change: e.change.map(change_name),
                })
                .collect(),
            more: folder.more,
            git: folder.git,
        })
        .collect();
    Ok(Tree {
        folders,
        truncated: tree.truncated,
    })
}

/// The first `TEXT_CAP` bytes of the file `path` in `root`, for the text view.
#[tauri::command]
pub async fn agent_tree_text(
    beep: Service<'_>,
    machine_id: String,
    root: String,
    path: String,
) -> Result<TreeText> {
    let part = read_part(
        &beep,
        ReadWorkFile {
            machine_id,
            root,
            path,
            offset: 0,
            len: TEXT_CAP,
        },
    )
    .await?;
    let have = u64::try_from(part.data.len()).map_err(anyhow::Error::from)?;
    let binary = looks_binary(&part.data);
    let text = if binary {
        String::new()
    } else {
        String::from_utf8_lossy(&part.data).into_owned()
    };
    Ok(TreeText {
        size: part.size,
        text,
        cut: have < part.size,
        binary,
    })
}

/// A NUL byte near the start says it is not text.
fn looks_binary(data: &[u8]) -> bool {
    data.iter().take(8192).any(|b| *b == 0)
}

/// Saves the file `path` in `root` to Downloads, under a name not taken
/// there yet, and returns where it went.
#[tauri::command]
pub async fn agent_tree_save(
    app: AppHandle,
    beep: Service<'_>,
    machine_id: String,
    root: String,
    path: String,
) -> Result<PathBuf> {
    let chunk = u32::try_from(AGENT_FILE_CHUNK).map_err(anyhow::Error::from)?;
    let mut data = Vec::new();
    loop {
        let offset = u64::try_from(data.len()).map_err(anyhow::Error::from)?;
        let part = read_part(
            &beep,
            ReadWorkFile {
                machine_id: machine_id.clone(),
                root: root.clone(),
                path: path.clone(),
                offset,
                len: chunk,
            },
        )
        .await?;
        if part.size > MAX_AGENT_FILE {
            return Err(DesktopError::BadRequest(format!(
                "It is over {} MB, too large to save from here.",
                MAX_AGENT_FILE / (1024 * 1024)
            )));
        }
        data.extend_from_slice(&part.data);
        let have = u64::try_from(data.len()).map_err(anyhow::Error::from)?;
        if have >= part.size {
            data.truncate(usize::try_from(part.size).map_err(anyhow::Error::from)?);
            break;
        }
        if part.data.is_empty() {
            return Err(DesktopError::BadRequest(
                "the file changed while read".into(),
            ));
        }
    }
    let to = free_name(&app.path().download_dir()?, &file_name(&path));
    fs::write(&to, &data).map_err(anyhow::Error::from)?;
    Ok(to)
}

/// Writes one part of a dropped file, `data` in base64, to `path` in
/// `root`; the path it goes to, which the next parts send.
#[tauri::command]
pub async fn agent_tree_upload(
    beep: Service<'_>,
    machine_id: String,
    root: String,
    path: String,
    offset: u64,
    size: u64,
    data: String,
) -> Result<String> {
    let data = STANDARD
        .decode(data.as_bytes())
        .map_err(|e| DesktopError::BadRequest(format!("the file's bytes did not read: {e}")))?;
    beep.call(WriteWorkFile {
        machine_id,
        root,
        path,
        offset,
        size,
        data,
    })
    .await
}
