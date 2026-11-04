//! Pictures and media files a coding-agent session refers to, read from
//! its machine through the server: shown, opened or saved here.

use crate::{
    beep::Beep,
    chat::files::{GONE, file_name, free_name, image_mime},
    error::{DesktopError, Result},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::{
    AGENT_FILE_CHUNK, AgentFileChunk, ApiError, GetAgentFile, GetAgentImage, MAX_AGENT_FILE,
    image_key,
};
use serde::Serialize;
use std::{fs, path::PathBuf, process::Command, sync::Arc};
use tauri::{AppHandle, Manager, State};

type Service<'a> = State<'a, Arc<Beep>>;

/// The picture of the `[image: image-<key>]` line of `session`, read from
/// its machine through the server, as a data URL the bar can draw.
#[tauri::command]
pub async fn agent_image(
    beep: Service<'_>,
    machine_id: String,
    session: String,
    key: String,
) -> Result<String> {
    let image = beep
        .call(GetAgentImage {
            machine_id,
            session,
            key,
        })
        .await;
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
/// from `offset`, read from its machine through the server.
#[tauri::command]
pub async fn agent_file(
    beep: Service<'_>,
    machine_id: String,
    path: String,
    offset: u64,
) -> Result<AgentFilePart> {
    let chunk = file_chunk(&beep, machine_id, path, offset).await?;
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
    machine_id: String,
    path: String,
) -> Result<()> {
    let data = whole_file(&beep, &machine_id, &path).await?;
    let dir = app.path().app_cache_dir()?.join("agent-files");
    fs::create_dir_all(&dir).map_err(anyhow::Error::from)?;
    let of = image_key(format!("{machine_id}\n{path}").as_bytes());
    let to = dir.join(format!("{of}-{}", file_name(&path)));
    fs::write(&to, &data).map_err(anyhow::Error::from)?;
    Command::new("open")
        .arg(&to)
        .spawn()
        .map_err(anyhow::Error::from)?;
    Ok(())
}

/// Saves the media file `path` on `machine_id` to Downloads, under a name
/// not taken there yet, and returns where it went.
#[tauri::command]
pub async fn agent_file_save(
    app: AppHandle,
    beep: Service<'_>,
    machine_id: String,
    path: String,
) -> Result<PathBuf> {
    let data = whole_file(&beep, &machine_id, &path).await?;
    let to = free_name(&app.path().download_dir()?, &file_name(&path));
    fs::write(&to, &data).map_err(anyhow::Error::from)?;
    Ok(to)
}

/// One `GetAgentFile`, with a file the machine no longer has as `GONE`.
async fn file_chunk(
    beep: &Beep,
    machine_id: String,
    path: String,
    offset: u64,
) -> Result<AgentFileChunk> {
    let chunk = beep
        .call(GetAgentFile {
            machine_id,
            path,
            offset,
        })
        .await;
    match chunk {
        Err(DesktopError::Api(ApiError::NotFound {})) => Err(DesktopError::BadRequest(GONE.into())),
        other => other,
    }
}

/// The whole media file `path` on `machine_id`, read a chunk at a time.
async fn whole_file(beep: &Beep, machine_id: &str, path: &str) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    loop {
        let offset = u64::try_from(data.len()).map_err(anyhow::Error::from)?;
        let chunk = file_chunk(beep, machine_id.to_owned(), path.to_owned(), offset).await?;
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
