//! The Files tab of a coding agent session: its working folder on the
//! session's machine listed a few levels at a time, a file read to show as
//! text or saved to Downloads, and files dropped on the session uploaded
//! into the folder a part at a time.

use crate::{
    agents::{files_live::FilesLive, work_copy},
    chat::files::{file_name, free_name, gone},
    error::{DesktopError, Result},
    fleet::{Fleet, Machines, files as direct},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_mesh::machine::sessions::Reading;
use beep_proto::SessionId;
use beep_proto::{
    AGENT_FILE_CHUNK, AgentFileChunk, GitChange, ListWorkTree, MAX_TREE_DEPTH, ReadWorkFile,
    WriteWorkFile,
};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use tauri::{AppHandle, State};

/// The most of a file the text view shows.
const TEXT_CAP: u32 = 512 * 1024;

/// One folder of the working folder, for the bar.
#[derive(Debug, Serialize)]
pub struct TreeFolder {
    path: String,
    entries: Vec<TreeEntry>,
    more: u32,
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
/// UTF-8, `cut` when there is more, `binary` when it does not read as text,
/// `editable` when `text` is all of it, unchanged by the reading.
#[derive(Debug, Serialize)]
pub struct TreeText {
    size: u64,
    text: String,
    cut: bool,
    binary: bool,
    editable: bool,
}

/// One `ReadWorkFile`, with a file the machine no longer has as `GONE`.
pub(crate) async fn read_part(fleet: &Fleet, read: ReadWorkFile) -> Result<AgentFileChunk> {
    let full = work_copy::joined(&read.root, &read.path);

    if let Some(part) = work_copy::read(fleet, &read.machine_id, &full, read.offset, read.len).await
    {
        return Ok(part);
    }

    let machine = fleet.machine(&read.machine_id)?;
    gone(direct::read(fleet, machine, &read).await)
}

/// The folders `TREE_DEPTH` levels down from `path`, for the bar.
#[derive(Debug, Serialize)]
pub struct Tree {
    folders: Vec<TreeFolder>,
    truncated: bool,
}

/// How many levels one read of the tree takes.
const TREE_DEPTH: u32 = 3;

// The arguments are the fields of the window's invoke call.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn agent_tree(
    fleet: Machines<'_>,
    live: State<'_, FilesLive>,
    machine_id: String,
    root: String,
    path: String,
    ignored: bool,
    deep: bool,
    fresh: Option<bool>,
) -> Result<Tree> {
    // Asking makes the window follow the folder: a change comes as a view.
    if let Some(machine) = fleet.member(&machine_id) {
        live.follow_tree(machine, &root, &path, ignored, deep);
    }

    tree_of(
        &fleet,
        &machine_id,
        &root,
        &path,
        ignored,
        deep,
        fresh.unwrap_or(false),
    )
    .await
}

/// What `agent_tree` answers, as the folder's views push it too.
pub(crate) async fn tree_of(
    fleet: &Fleet,
    machine_id: &str,
    root: &str,
    path: &str,
    ignored: bool,
    deep: bool,
    fresh: bool,
) -> Result<Tree> {
    let ask = ListWorkTree {
        machine_id: machine_id.to_owned(),
        root: root.to_owned(),
        path: path.to_owned(),
        ignored,
        depth: if deep { MAX_TREE_DEPTH } else { TREE_DEPTH },
    };
    let cached = if fresh {
        None
    } else {
        work_copy::tree(fleet, &ask).await
    };
    let tree = match cached {
        Some(tree) => tree,
        None => {
            let machine = fleet.machine(&ask.machine_id)?;
            direct::tree(fleet, machine, &ask, Reading::Followed).await?
        }
    };
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
        })
        .collect();
    Ok(Tree {
        folders,
        truncated: tree.truncated,
    })
}

/// The BLAKE3 hash of each of `paths` in `root`, in base64, from the copy
/// reads come from: `None` for a file not there, and `None` in all from a
/// node from before the question, which the pane then checks by size.
#[tauri::command]
pub async fn agent_file_hashes(
    fleet: Machines<'_>,
    machine_id: String,
    root: String,
    paths: Vec<String>,
) -> Result<Option<Vec<Option<String>>>> {
    let hashes = match work_copy::hashes(&fleet, &machine_id, &root, paths.clone()).await {
        Some(hashes) => Some(hashes),
        None => {
            let machine = fleet.machine(&machine_id)?;
            direct::hashes(&fleet, machine, &root, paths).await?
        }
    };
    Ok(hashes.map(|hashes| {
        hashes
            .into_iter()
            .map(|hash| hash.map(|hash| STANDARD.encode(hash)))
            .collect()
    }))
}

/// The first `TEXT_CAP` bytes of the file `path` in `root`, for the text view.
#[tauri::command]
pub async fn agent_tree_text(
    fleet: Machines<'_>,
    machine_id: String,
    root: String,
    path: String,
) -> Result<TreeText> {
    let part = read_part(
        &fleet,
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
    let (cut, binary) = (have < part.size, looks_binary(&part.data));
    let (text, exact) = if binary {
        (String::new(), false)
    } else {
        match String::from_utf8(part.data) {
            Ok(text) => (text, true),
            Err(e) => (String::from_utf8_lossy(e.as_bytes()).into_owned(), false),
        }
    };
    Ok(TreeText {
        size: part.size,
        text,
        cut,
        binary,
        editable: exact && !cut,
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
    fleet: Machines<'_>,
    machine_id: String,
    root: String,
    path: String,
) -> Result<PathBuf> {
    let chunk = u32::try_from(AGENT_FILE_CHUNK).map_err(anyhow::Error::from)?;
    let mut data = Vec::new();

    loop {
        let offset = u64::try_from(data.len()).map_err(anyhow::Error::from)?;
        let part = read_part(
            &fleet,
            ReadWorkFile {
                machine_id: machine_id.clone(),
                root: root.clone(),
                path: path.clone(),
                offset,
                len: chunk,
            },
        )
        .await?;

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

    let to = free_name(&crate::platform::saves_dir(&app)?, &file_name(&path));
    fs::write(&to, &data).map_err(anyhow::Error::from)?;
    Ok(to)
}

/// The session owning a temporary attachment on its host.
#[derive(Deserialize)]
pub struct AttachmentSession {
    agent: String,
    session: SessionId,
}

/// Writes one part of a dropped file, `data` in base64, to `path` in
/// `root`; the path it goes to, which the next parts send.
// The arguments are the fields of the window's `invoke` call.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn agent_tree_upload(
    fleet: Machines<'_>,
    machine_id: String,
    root: String,
    path: String,
    offset: u64,
    size: u64,
    data: String,
    attachment: Option<AttachmentSession>,
) -> Result<String> {
    let data = STANDARD
        .decode(data.as_bytes())
        .map_err(|e| DesktopError::BadRequest(format!("the file's bytes did not read: {e}")))?;
    let write = WriteWorkFile {
        machine_id,
        root,
        path,
        offset,
        size,
        data,
    };

    let machine = fleet.machine(&write.machine_id)?;

    match attachment {
        Some(owner) => direct::upload(&fleet, machine, owner.agent, owner.session, write).await,
        None => direct::write(&fleet, machine, write).await,
    }
}

/// Removes a temporary upload from a new chat's draft.
#[tauri::command]
pub async fn agent_discard_draft_attachment(
    fleet: Machines<'_>,
    machine_id: String,
    draft: SessionId,
    path: String,
) -> Result<()> {
    direct::discard_draft_attachment(&fleet, fleet.machine(&machine_id)?, draft, path).await
}

/// File browser edits require a direct mesh connection to the owning machine.
#[tauri::command]
pub async fn agent_tree_edit(
    fleet: Machines<'_>,
    machine_id: String,
    root: String,
    action: beep_proto::node::agent::FileAction,
) -> Result<()> {
    let machine = fleet.member(&machine_id).ok_or_else(|| {
        DesktopError::BadRequest("Connect this machine to the mesh to edit its files".into())
    })?;
    direct::edit_files(&fleet, machine, root, action).await
}

/// Resolves code definitions on the owning machine via an installed language server.
#[tauri::command]
pub async fn agent_lsp_definition(
    fleet: Machines<'_>,
    machine_id: String,
    root: String,
    path: String,
    line: u32,
    column: u32,
    text: Option<String>,
) -> Result<beep_proto::node::agent::LspDefinitions> {
    let rel = beep_proto::node::agent::RelPath::try_from(path)
        .map_err(|e| DesktopError::BadRequest(e.to_string()))?;
    let machine = fleet.member(&machine_id).ok_or_else(|| {
        DesktopError::BadRequest("Connect this machine to the mesh to look up definitions".into())
    })?;
    direct::lsp_definition(&fleet, machine, root, rel, line, column, text).await
}

/// Saves a selection, including folders, under one new folder in Downloads.
#[tauri::command]
pub async fn agent_tree_save_many(
    app: AppHandle,
    fleet: Machines<'_>,
    machine_id: String,
    root: String,
    paths: Vec<String>,
) -> Result<PathBuf> {
    use std::{
        collections::{HashMap, HashSet, VecDeque},
        io::Write,
        path::{Component, Path},
    };

    if paths.is_empty() || paths.len() > 50_000 {
        return Err(DesktopError::BadRequest(
            "Select between 1 and 50000 items".into(),
        ));
    }

    let machine = fleet.member(&machine_id).ok_or_else(|| {
        DesktopError::BadRequest("Connect this machine to the mesh to download folders".into())
    })?;
    let valid = |path: &str| {
        !path.is_empty()
            && path
                .split('/')
                .all(|p| !p.is_empty() && p != "." && p != ".." && !p.contains(['\\', ':']))
            && Path::new(path)
                .components()
                .all(|p| matches!(p, Component::Normal(_)))
    };

    if paths.iter().any(|p| !valid(p)) {
        return Err(DesktopError::BadRequest("Invalid selection path".into()));
    }

    let to = free_name(&crate::platform::saves_dir(&app)?, "Files");
    fs::create_dir(&to).map_err(anyhow::Error::from)?;
    let selected: HashSet<_> = paths.iter().cloned().collect();
    let mut queue: VecDeque<String> = paths
        .into_iter()
        .filter(|path| {
            !Path::new(path)
                .ancestors()
                .skip(1)
                .any(|parent| parent.to_str().is_some_and(|p| selected.contains(p)))
        })
        .collect();
    let mut seen = HashSet::new();
    let mut listings = HashMap::new();
    let mut total = 0u64;
    let (fleet_ref, machine_id_ref, root_ref) = (&fleet, &machine_id, &root);
    let list = |path: String| async move {
        let listing = direct::tree(
            fleet_ref,
            machine,
            &ListWorkTree {
                machine_id: machine_id_ref.clone(),
                root: root_ref.clone(),
                path,
                ignored: true,
                depth: 1,
            },
            Reading::Once,
        )
        .await?;
        let folder = listing
            .folders
            .into_iter()
            .next()
            .ok_or_else(|| DesktopError::BadRequest("The folder could not be read".into()))?;

        if folder.more > 0 {
            return Err(DesktopError::BadRequest(
                "The folder has too many entries to download completely".into(),
            ));
        }

        Ok::<_, DesktopError>(folder)
    };

    while let Some(path) = queue.pop_front() {
        if !seen.insert(path.clone()) {
            continue;
        }

        if seen.len() > 50_000 || path.split('/').count() > 128 {
            return Err(DesktopError::BadRequest(
                "Download at most 50000 items at once".into(),
            ));
        }

        let parent = path.rsplit_once('/').map_or("", |(p, _)| p);

        if !listings.contains_key(parent) {
            listings.insert(parent.to_owned(), list(parent.into()).await?);
        }

        let top = listings
            .get(parent)
            .ok_or_else(|| DesktopError::BadRequest("The folder could not be read".into()))?;

        let entry = top
            .entries
            .iter()
            .find(|e| e.name == path.rsplit('/').next().unwrap_or_default())
            .ok_or_else(|| DesktopError::BadRequest("The item is no longer there".into()))?;
        let dest = to.join(&path);
        if entry.folder {
            fs::create_dir_all(&dest).map_err(anyhow::Error::from)?;
            let folder = list(path.clone()).await?;

            if queue
                .len()
                .saturating_add(seen.len())
                .saturating_add(folder.entries.len())
                > 50_000
            {
                return Err(DesktopError::BadRequest(
                    "Download at most 50000 items at once".into(),
                ));
            }

            for child in &folder.entries {
                if !valid(&child.name) || child.name.contains('/') {
                    return Err(DesktopError::BadRequest("Invalid file name".into()));
                }

                queue.push_back(format!("{path}/{}", child.name));
            }

            listings.insert(path, folder);
        } else {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(anyhow::Error::from)?;
            }

            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&dest)
                .map_err(anyhow::Error::from)?;
            let mut offset = 0;
            let mut size = None;

            loop {
                let part = direct::read(
                    &fleet,
                    machine,
                    &ReadWorkFile {
                        machine_id: machine_id.clone(),
                        root: root.clone(),
                        path: path.clone(),
                        offset,
                        len: u32::try_from(AGENT_FILE_CHUNK).map_err(anyhow::Error::from)?,
                    },
                )
                .await?;

                let count = u64::try_from(part.data.len()).map_err(anyhow::Error::from)?;

                if size.is_some_and(|size| size != part.size) || offset + count > part.size {
                    return Err(DesktopError::BadRequest(
                        "The file changed while downloading".into(),
                    ));
                }
                size = Some(part.size);
                total = total
                    .checked_add(count)
                    .ok_or_else(|| DesktopError::BadRequest("The download is too large".into()))?;
                if total > 256 * 1024 * 1024 {
                    return Err(DesktopError::BadRequest(
                        "Download at most 256 MB at once".into(),
                    ));
                }

                file.write_all(&part.data).map_err(anyhow::Error::from)?;
                offset += count;

                if offset >= part.size {
                    break;
                }

                if count == 0 {
                    return Err(DesktopError::BadRequest(
                        "The file changed while downloading".into(),
                    ));
                }
            }
        }
    }

    Ok(to)
}
