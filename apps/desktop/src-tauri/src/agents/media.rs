//! Pictures and media files a coding-agent session refers to, read from
//! its machine: shown, opened or saved here.

use crate::agents::file_cache::{Cached, FileCache, KEEP, MAX_BYTES};
use crate::{
    chat::files::{file_name, free_name, gone, image_mime},
    error::{DesktopError, Result},
    fleet::{AsyncMutex, Fleet, Machines, agents as direct, work_copy},
};
use base64::{Engine, engine::general_purpose::STANDARD};
#[cfg(desktop)]
use beep_mesh::machine::thumb::{Picture, thumb_of};
use beep_proto::media_mime;
use beep_proto::{
    AGENT_FILE_CHUNK, AgentFileChunk, AgentImageData, GetAgentFile, GetAgentImage, SessionId,
    image_key,
};
use iroh::EndpointId;
use serde::Serialize;
use std::sync::Arc;
use std::{
    future::Future,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use tauri::Emitter;
use tauri::{AppHandle, Manager};
use tempfile::{NamedTempFile, TempPath};
use tokio::io::AsyncWriteExt;

/// Run once at startup, then age out old media while the app runs.
pub(crate) const SWEEP_EVERY: Duration = Duration::from_secs(30 * 60);

// A sweep must not remove a copy replaced after its metadata was read.
static OPEN_COPIES: AsyncMutex<()> = AsyncMutex::const_new(());

/// The picture of the `[image: image-<key>]` line of `session`, read from
/// its machine, as a data URL the bar can draw.
#[tauri::command]
pub async fn agent_image(
    fleet: Machines<'_>,
    machine_id: String,
    session: SessionId,
    key: String,
) -> Result<String> {
    let machine = fleet.machine(&machine_id)?;
    let request = GetAgentImage {
        machine_id,
        session,
        key,
    };
    picture_url(direct::image(&fleet, machine, &request).await)
}

/// `agent_image`'s picture made small, for the log to draw. Never for opening
/// or saving.
#[tauri::command]
pub async fn agent_thumb(
    fleet: Machines<'_>,
    machine_id: String,
    session: SessionId,
    key: String,
) -> Result<String> {
    // The machine makes it small; one from before that sends it whole, and
    // it is made small here.
    let machine = fleet.machine(&machine_id)?;
    let request = GetAgentImage {
        machine_id,
        session,
        key,
    };
    let image = match direct::thumb(&fleet, machine, &request).await {
        Ok(None) => match direct::image(&fleet, machine, &request).await {
            Ok(image) => Ok(small(image).await),
            Err(e) => Err(e),
        },
        Ok(Some(thumb)) => Ok(thumb),
        Err(e) => Err(e),
    };
    picture_url(image)
}

/// `image` made small, for the log to draw.
#[cfg(desktop)]
async fn small(image: AgentImageData) -> AgentImageData {
    let made = thumb_of(Picture {
        mime: image.mime,
        data: image.data,
    })
    .await;
    AgentImageData {
        key: image.key,
        mime: made.mime,
        data: made.data,
    }
}

/// A phone draws a picture from an older machine as it comes.
#[cfg(mobile)]
async fn small(image: AgentImageData) -> AgentImageData {
    image
}

/// A data URL the bar can draw, with a picture the machine no longer has as
/// `GONE`.
fn picture_url(image: Result<AgentImageData>) -> Result<String> {
    let image = gone(image)?;

    if !image_mime(&image.mime) {
        return Err(DesktopError::BadRequest("bad picture".into()));
    }

    Ok(format!(
        "data:{};base64,{}",
        image.mime,
        STANDARD.encode(&image.data)
    ))
}

/// Part of a media file on a machine, for the bar's media pane: `data`
/// is base64, and the bar asks again from `offset` plus its length until
/// it has `size` bytes.
#[derive(Debug, Serialize)]
pub struct AgentFilePart {
    size: u64,
    mime: String,
    data: String,
}

/// Up to `AGENT_FILE_CHUNK` bytes of the media file `path` on `machine_id`
/// from `offset`. The first part fetches a remote file into the chat store,
/// kept for `session`; `agent-file-progress` tells how the download goes.
/// Files from older machines are read a part at a time.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn agent_file(
    app: AppHandle,
    fleet: Machines<'_>,
    machine_id: String,
    path: String,
    offset: u64,
    session: Option<SessionId>,
) -> Result<AgentFilePart> {
    let cache = app.state::<Arc<FileCache>>();
    let asked = Asked {
        machine_id: &machine_id,
        path: &path,
        session: session.as_ref(),
    };
    // Later parts come from what the first one fetched, or the node.
    let cached = if offset == 0 {
        cached(&app, &fleet, &cache, asked).await?
    } else {
        remote(&fleet, &machine_id).and_then(|machine| cache.known(machine, &path))
    };

    if let Some(cached) = cached {
        let data = cache.read(cached, offset, AGENT_FILE_CHUNK).await?;
        return Ok(AgentFilePart {
            size: cached.size,
            mime: media_mime(&path)
                .unwrap_or("application/octet-stream")
                .to_owned(),
            data: STANDARD.encode(&data),
        });
    }

    let chunk = file_chunk(&fleet, machine_id, path, offset).await?;
    Ok(AgentFilePart {
        size: chunk.size,
        mime: chunk.mime,
        data: STANDARD.encode(&chunk.data),
    })
}

/// Opens the media file `path` on `machine_id` in this computer's own app,
/// after downloading a complete local copy.
#[tauri::command]
pub async fn agent_file_open(
    app: AppHandle,
    fleet: Machines<'_>,
    machine_id: String,
    path: String,
    session: Option<SessionId>,
) -> Result<()> {
    let cache = app.state::<Arc<FileCache>>();
    let asked = Asked {
        machine_id: &machine_id,
        path: &path,
        session: session.as_ref(),
    };

    if let Some(cached) = cached(&app, &fleet, &cache, asked).await? {
        let to = cache.copy_path(cached, &file_name(&path));
        cache.export(cached, &to).await?;
        crate::platform::open(&to)?;
        return Ok(());
    }

    let dir = app.path().app_cache_dir()?.join("agent-files");
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(anyhow::Error::from)?;
    let of = image_key(format!("{machine_id}\n{path}").as_bytes());
    let to = dir.join(format!("{of}-{}", file_name(&path)));
    let temporary = download_file(&dir, |offset| {
        file_chunk(&fleet, machine_id.clone(), path.clone(), offset)
    })
    .await?;
    let _one = OPEN_COPIES.lock().await;
    temporary.persist(&to).map_err(anyhow::Error::from)?;
    trim_open_files(&dir, Some(&to)).await;
    crate::platform::open(&to)?;
    Ok(())
}

/// Saves the media file `path` on `machine_id` to Downloads, under a name
/// not taken there yet, and returns where it went.
#[tauri::command]
pub async fn agent_file_save(
    app: AppHandle,
    fleet: Machines<'_>,
    machine_id: String,
    path: String,
    session: Option<SessionId>,
) -> Result<PathBuf> {
    let to = free_name(&crate::platform::saves_dir(&app)?, &file_name(&path));
    let cache = app.state::<Arc<FileCache>>();
    let asked = Asked {
        machine_id: &machine_id,
        path: &path,
        session: session.as_ref(),
    };

    if let Some(cached) = cached(&app, &fleet, &cache, asked).await? {
        cache.export(cached, &to).await?;
        return Ok(to);
    }

    let dir = to
        .parent()
        .ok_or_else(|| DesktopError::BadRequest("bad save folder".into()))?;
    let temporary = download_file(dir, |offset| {
        file_chunk(&fleet, machine_id.clone(), path.clone(), offset)
    })
    .await?;
    temporary
        .persist_noclobber(&to)
        .map_err(anyhow::Error::from)?;
    Ok(to)
}

/// Conversation `session` closed: drops the files only it opened.
#[tauri::command]
pub async fn agent_files_forget(app: AppHandle, session: SessionId) -> Result<()> {
    app.state::<Arc<FileCache>>().forget(&session).await?;
    Ok(())
}

/// A media file asked for, from the conversation `session` when there is one.
#[derive(Clone, Copy)]
struct Asked<'a> {
    machine_id: &'a str,
    path: &'a str,
    session: Option<&'a SessionId>,
}

/// How far fetching a chat file has come, as `agent-file-progress` tells it.
#[derive(Clone, Serialize)]
struct FileProgress<'a> {
    machine_id: &'a str,
    path: &'a str,
    done: u64,
    size: u64,
}

/// Another mesh machine, whose files the chat file store keeps.
fn remote(fleet: &Fleet, machine_id: &str) -> Option<EndpointId> {
    fleet
        .member(machine_id)
        .filter(|machine| *machine != fleet.id())
}

/// `asked` from the chat file store, fetched unless it holds the file
/// already. `None` for a file read another way: on this machine, in a
/// working copy here, or from a node that cannot share.
async fn cached(
    app: &AppHandle,
    fleet: &Fleet,
    cache: &FileCache,
    asked: Asked<'_>,
) -> Result<Option<Cached>> {
    let Some(machine) = remote(fleet, asked.machine_id) else {
        return Ok(None);
    };

    // A copy of its project here is read as it is.
    if work_copy::read(fleet, asked.machine_id, asked.path, 0, 1)
        .await
        .is_some()
    {
        return Ok(None);
    }

    let tell = |done, size| {
        let progress = FileProgress {
            machine_id: asked.machine_id,
            path: asked.path,
            done,
            size,
        };

        if let Err(e) = app.emit("agent-file-progress", progress) {
            log::debug!("could not tell a file's progress: {e}");
        }
    };
    cache
        .fetch(fleet, machine, asked.path, asked.session, tell)
        .await
}

/// One `GetAgentFile`, with a file the machine no longer has as `GONE`.
async fn file_chunk(
    fleet: &Fleet,
    machine_id: String,
    path: String,
    offset: u64,
) -> Result<AgentFileChunk> {
    let len = u32::try_from(AGENT_FILE_CHUNK).map_err(anyhow::Error::from)?;

    if let Some(chunk) = work_copy::read(fleet, &machine_id, &path, offset, len).await {
        return Ok(chunk);
    }

    let machine = fleet.machine(&machine_id)?;
    let request = GetAgentFile {
        machine_id,
        path,
        offset,
    };
    gone(direct::file(fleet, machine, &request).await)
}

/// Bounds the completed copies kept for native viewers, including copies
/// left by a previous app run. In-progress temporary files stay untouched.
pub(crate) async fn sweep_open_files(dir: &Path) {
    let _one = OPEN_COPIES.lock().await;
    trim_open_files(dir, None).await;
}

async fn trim_open_files(dir: &Path, keep: Option<&Path>) {
    if let Err(error) = prune_open_files(dir, keep, SystemTime::now(), MAX_BYTES).await {
        log::warn!("dropping old media copies failed: {error}");
    }
}

struct OpenCopy {
    path: PathBuf,
    size: u64,
    opened: SystemTime,
}

async fn prune_open_files(
    dir: &Path,
    keep: Option<&Path>,
    now: SystemTime,
    budget: u64,
) -> std::io::Result<()> {
    let metadata = match tokio::fs::symlink_metadata(dir).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_dir() {
        return Ok(());
    }

    let mut entries = tokio::fs::read_dir(dir).await?;
    let mut copies = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let Some((key, name)) = name.to_str().and_then(|name| name.split_once('-')) else {
            continue;
        };
        if key.len() != 16
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || name.is_empty()
        {
            continue;
        }
        let metadata = tokio::fs::symlink_metadata(entry.path()).await?;
        if metadata.file_type().is_file() {
            copies.push(OpenCopy {
                path: entry.path(),
                size: metadata.len(),
                opened: metadata.modified()?,
            });
        }
    }
    copies.sort_by_key(|copy| std::cmp::Reverse(copy.opened));

    let expired = |copy: &OpenCopy| now.duration_since(copy.opened).unwrap_or_default() > KEEP;
    // External viewers may still read the newest copy. Keep it until it ages
    // out, even when that one file exceeds the normal store budget.
    let keep = copies
        .iter()
        .find(|copy| Some(copy.path.as_path()) == keep)
        .or_else(|| copies.first().filter(|copy| !expired(copy)))
        .map(|copy| copy.path.clone());
    let mut held = copies
        .iter()
        .find(|copy| Some(&copy.path) == keep.as_ref())
        .map_or(0, |copy| copy.size);

    for copy in copies {
        if Some(&copy.path) == keep.as_ref() {
            continue;
        }
        if expired(&copy) || copy.size > budget.saturating_sub(held) {
            match tokio::fs::remove_file(copy.path).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        } else {
            held = held.saturating_add(copy.size);
        }
    }
    Ok(())
}

/// Downloads a file in bounded chunks. The guarded temporary file disappears
/// on errors and cancellation; callers only publish complete downloads.
async fn download_file<F>(dir: &Path, mut read: impl FnMut(u64) -> F + Send) -> Result<TempPath>
where
    F: Future<Output = Result<AgentFileChunk>> + Send,
{
    let dir = dir.to_owned();
    let temporary = tokio::task::spawn_blocking(move || NamedTempFile::new_in(dir))
        .await
        .map_err(anyhow::Error::from)?
        .map_err(anyhow::Error::from)?;
    let (file, temporary) = temporary.into_parts();
    let mut file = tokio::fs::File::from_std(file);
    let mut offset = 0_u64;
    let mut size = None;

    loop {
        let chunk = read(offset).await?;
        let len = u64::try_from(chunk.data.len()).map_err(anyhow::Error::from)?;
        let next = offset
            .checked_add(len)
            .ok_or_else(|| DesktopError::BadRequest("the file size is out of range".into()))?;

        if chunk.data.len() > AGENT_FILE_CHUNK {
            return Err(DesktopError::BadRequest("file chunk too large".into()));
        }
        if size.is_some_and(|size| chunk.size != size)
            || next > chunk.size
            || (len == 0 && offset < chunk.size)
        {
            return Err(DesktopError::BadRequest(
                "the file changed while read".into(),
            ));
        }

        size = Some(chunk.size);
        file.write_all(&chunk.data)
            .await
            .map_err(anyhow::Error::from)?;
        offset = next;

        if offset == chunk.size {
            file.flush().await.map_err(anyhow::Error::from)?;
            drop(file);
            return Ok(temporary);
        }
    }
}

#[cfg(test)]
#[path = "../tests/agents_media.rs"]
mod tests;
