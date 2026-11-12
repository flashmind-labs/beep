//! Pictures and media files a coding-agent session refers to, read from
//! its machine through the server: shown, opened or saved here.

use crate::{
    agents::{
        file_cache::{Cached, FileCache},
        work_copy,
    },
    beep::{Beep, Service},
    chat::files::{GONE, file_name, free_name, image_mime},
    error::{DesktopError, Result},
    fleet::{Fleet, Machines, agents as direct},
    projects::unknown_op,
};
use base64::{Engine, engine::general_purpose::STANDARD};
#[cfg(desktop)]
use beep_mesh::machine::thumb::{Picture, thumb_of};
use beep_proto::{
    AGENT_FILE_CHUNK, AgentFileChunk, AgentImageData, ApiError, GetAgentFile, GetAgentImage,
    GetAgentThumb, MAX_AGENT_FILE, image_key, media_mime,
};
use iroh::EndpointId;
use serde::Serialize;
use std::{fs, path::PathBuf, sync::Arc};
use tauri::{AppHandle, Emitter, Manager, State};

/// The chat file store, as commands take it.
type Files<'a> = State<'a, Arc<FileCache>>;

/// The picture of the `[image: image-<key>]` line of `session`, read from
/// its machine through the server, as a data URL the bar can draw.
#[tauri::command]
pub async fn agent_image(
    beep: Service<'_>,
    fleet: Machines<'_>,
    machine_id: String,
    session: String,
    key: String,
) -> Result<String> {
    let request = GetAgentImage {
        machine_id,
        session,
        key,
    };
    let image = match fleet.member(&request.machine_id) {
        Some(machine) => direct::image(&fleet, machine, &request).await,
        None => beep.call(request).await,
    };
    picture_url(image)
}

/// `agent_image`'s picture made small, for the log to draw. Never for opening
/// or saving: a server from before thumbnails gives it whole, but a newer
/// one gives a small JPEG.
#[tauri::command]
pub async fn agent_thumb(
    beep: Service<'_>,
    fleet: Machines<'_>,
    machine_id: String,
    session: String,
    key: String,
) -> Result<String> {
    // The machine makes it small; one from before that sends it whole, and
    // it is made small here.
    if let Some(machine) = fleet.member(&machine_id) {
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
        return picture_url(image);
    }

    let thumb = beep
        .call(GetAgentThumb {
            machine_id: machine_id.clone(),
            session: session.clone(),
            key: key.clone(),
        })
        .await;
    let image = match thumb {
        Err(e) if unknown_op(&e) => {
            beep.call(GetAgentImage {
                machine_id,
                session,
                key,
            })
            .await
        }
        other => other,
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

/// A data URL the bar can draw, with a picture the server no longer has as
/// `GONE`.
fn picture_url(image: Result<AgentImageData>) -> Result<String> {
    let image = match image {
        Err(DesktopError::Api(ApiError::NotFound {})) => {
            return Err(DesktopError::BadRequest(GONE.into()));
        }
        other => other?,
    };

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
/// from `offset`. Another mesh machine's file comes from the chat file
/// store, fetched whole with the first part and kept for `session`, and
/// `agent-file-progress` events tell how the download goes; otherwise it is
/// read from its machine or through the server.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn agent_file(
    app: AppHandle,
    beep: Service<'_>,
    fleet: Machines<'_>,
    cache: Files<'_>,
    machine_id: String,
    path: String,
    offset: u64,
    session: Option<String>,
) -> Result<AgentFilePart> {
    let asked = Asked {
        machine_id: &machine_id,
        path: &path,
        session: session.as_deref(),
    };
    // Later parts come from what the first one fetched, or the node.
    let cached = if offset == 0 {
        cached(&app, &beep, &fleet, &cache, asked).await?
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

    let chunk = file_chunk(&beep, &fleet, machine_id, path, offset).await?;
    Ok(AgentFilePart {
        size: chunk.size,
        mime: chunk.mime,
        data: STANDARD.encode(&chunk.data),
    })
}

/// Opens the media file `path` on `machine_id` in the Mac's own app for
/// it, read whole into the cache first.
#[tauri::command]
pub async fn agent_file_open(
    app: AppHandle,
    beep: Service<'_>,
    fleet: Machines<'_>,
    cache: Files<'_>,
    machine_id: String,
    path: String,
    session: Option<String>,
) -> Result<()> {
    let asked = Asked {
        machine_id: &machine_id,
        path: &path,
        session: session.as_deref(),
    };

    if let Some(cached) = cached(&app, &beep, &fleet, &cache, asked).await? {
        let to = cache.copy_path(cached, &file_name(&path));
        cache.export(cached, &to).await?;
        crate::platform::open(&to)?;
        return Ok(());
    }

    let data = whole_file(&beep, &fleet, &machine_id, &path).await?;
    let dir = app.path().app_cache_dir()?.join("agent-files");
    fs::create_dir_all(&dir).map_err(anyhow::Error::from)?;
    let of = image_key(format!("{machine_id}\n{path}").as_bytes());
    let to = dir.join(format!("{of}-{}", file_name(&path)));
    fs::write(&to, &data).map_err(anyhow::Error::from)?;
    crate::platform::open(&to)?;
    Ok(())
}

/// Saves the media file `path` on `machine_id` to Downloads, under a name
/// not taken there yet, and returns where it went.
#[tauri::command]
pub async fn agent_file_save(
    app: AppHandle,
    beep: Service<'_>,
    fleet: Machines<'_>,
    cache: Files<'_>,
    machine_id: String,
    path: String,
    session: Option<String>,
) -> Result<PathBuf> {
    let asked = Asked {
        machine_id: &machine_id,
        path: &path,
        session: session.as_deref(),
    };
    let to = free_name(&crate::platform::saves_dir(&app)?, &file_name(&path));

    if let Some(cached) = cached(&app, &beep, &fleet, &cache, asked).await? {
        cache.export(cached, &to).await?;
        return Ok(to);
    }

    let data = whole_file(&beep, &fleet, &machine_id, &path).await?;
    fs::write(&to, &data).map_err(anyhow::Error::from)?;
    Ok(to)
}

/// Conversation `session` closed: drops the files only it opened.
#[tauri::command]
pub async fn agent_files_forget(cache: Files<'_>, session: String) -> Result<()> {
    cache.forget(&session).await?;
    Ok(())
}

/// A media file asked for, from the conversation `session` when there is one.
#[derive(Clone, Copy)]
struct Asked<'a> {
    machine_id: &'a str,
    path: &'a str,
    session: Option<&'a str>,
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
/// working copy here, through the server, or from a node that cannot share.
async fn cached(
    app: &AppHandle,
    beep: &Beep,
    fleet: &Fleet,
    cache: &FileCache,
    asked: Asked<'_>,
) -> Result<Option<Cached>> {
    let Some(machine) = remote(fleet, asked.machine_id) else {
        return Ok(None);
    };

    // A copy of its project here is read as it is.
    if work_copy::read(beep, fleet, asked.machine_id, asked.path, 0, 1)
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
    beep: &Beep,
    fleet: &Fleet,
    machine_id: String,
    path: String,
    offset: u64,
) -> Result<AgentFileChunk> {
    let len = u32::try_from(AGENT_FILE_CHUNK).map_err(anyhow::Error::from)?;

    if let Some(chunk) = work_copy::read(beep, fleet, &machine_id, &path, offset, len).await {
        return Ok(chunk);
    }

    let request = GetAgentFile {
        machine_id,
        path,
        offset,
    };
    let chunk = match fleet.member(&request.machine_id) {
        Some(machine) => direct::file(fleet, machine, &request).await,
        None => beep.call(request).await,
    };

    match chunk {
        Err(DesktopError::Api(ApiError::NotFound {})) => Err(DesktopError::BadRequest(GONE.into())),
        other => other,
    }
}

/// The whole media file `path` on `machine_id`, read a chunk at a time.
async fn whole_file(beep: &Beep, fleet: &Fleet, machine_id: &str, path: &str) -> Result<Vec<u8>> {
    let mut data = Vec::new();

    loop {
        let offset = u64::try_from(data.len()).map_err(anyhow::Error::from)?;
        let chunk = file_chunk(beep, fleet, machine_id.to_owned(), path.to_owned(), offset).await?;

        if chunk.size > MAX_AGENT_FILE || chunk.data.len() > AGENT_FILE_CHUNK {
            return Err(DesktopError::BadRequest("file too large".into()));
        }

        data.extend_from_slice(&chunk.data);
        let have = u64::try_from(data.len()).map_err(anyhow::Error::from)?;

        if have >= chunk.size {
            data.truncate(usize::try_from(chunk.size).map_err(anyhow::Error::from)?);
            return Ok(data);
        }

        if chunk.data.is_empty() {
            return Err(DesktopError::BadRequest(
                "the file changed while read".into(),
            ));
        }
    }
}
