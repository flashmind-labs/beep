//! Temporary attachments on this host, outside project checkouts.

use beep_proto::SessionId;
use super::work_folder::{self, UNUSED, WorkRoot, idle, prune};
use super::Agents;
use crate::{files::paths, lock};
use anyhow::{Context, Result, bail};
use beep_proto::node::agent::Status;
use beep_proto::watched;
use beep_proto::AGENT_FILE_CHUNK;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{ErrorKind, Read},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};
use tokio_util::task::AbortOnDropHandle;
use uuid::Uuid;

const DAY: Duration = Duration::from_secs(24 * 60 * 60);
const SWEEP_EVERY: Duration = Duration::from_secs(60 * 60);
/// In a session's attachment folder: when the session last ended.
const ENDED: &str = "ended";
const DRAFT: &str = "draft";
// Chunks and cleanup must see one another's disk changes.
static WRITING: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize)]
struct Upload {
    size: u64,
    name: String,
}

pub(super) fn folder() -> Option<PathBuf> {
    dirs::cache_dir().map(|dir| dir.join("beep-node").join("attachments"))
}

pub(super) fn is_draft(agent: &str, session: &SessionId) -> bool {
    agent == DRAFT && Uuid::parse_str(session.as_ref()).is_ok()
}

/// Cleanup runs hourly from the first hour on, so sessions are back after a
/// restart, and stops with the node.
pub(crate) fn maintain(agents: Arc<Agents>) -> AbortOnDropHandle<()> {
    AbortOnDropHandle::new(tokio::spawn(async move {
        let start = tokio::time::Instant::now() + SWEEP_EVERY;
        let mut every = tokio::time::interval_at(start, SWEEP_EVERY);
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            every.tick().await;
            let (running, folders) = sessions(&agents);
            let done = tokio::task::spawn_blocking(move || -> Result<()> {
                let now = SystemTime::now();

                for folder in folders {
                    prune(&work_folder::uploads(&folder), now)?;
                }

                let _writing = lock(&WRITING);

                if let Some(cache) = folder() {
                    sweep(&cache, now, &running)?;
                }

                Ok(())
            })
            .await;

            if let Err(e) = done.unwrap_or_else(|e| Err(e.into())) {
                log::warn!("could not clean temporary attachments: {e:#}");
            }
        }
    }))
}

/// The attachment owners of the sessions still running, and the folders
/// every known session works in.
fn sessions(agents: &Agents) -> (HashSet<String>, HashSet<PathBuf>) {
    let mut running = HashSet::new();
    let mut folders = HashSet::new();

    for session in agents.session_map().values() {
        watched::read(&session.live, |live| {
            if live.status() != Status::Ended {
                running.insert(owner(&live.info.agent, &live.info.id));
            }
            folders.insert(PathBuf::from(&live.info.cwd));
        });
    }

    (running, folders)
}

/// Marks the session's attachments to go a day from now unless it runs again.
pub(super) fn ended(agent: &str, session: &SessionId) -> Result<()> {
    let Some(cache) = folder() else { return Ok(()) };
    let _writing = lock(&WRITING);
    let group = cache.join(owner(agent, session));

    match fs::symlink_metadata(&group) {
        Ok(meta) if meta.is_dir() => Ok(fs::write(group.join(ENDED), b"")?),
        Ok(_) => bail!("the session attachment folder is not a directory"),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

pub(super) fn write(
    agent: &str,
    session: &SessionId,
    path: &str,
    offset: u64,
    size: u64,
    data: &[u8],
    running: Option<&super::Session>,
) -> Result<String> {
    let _writing = lock(&WRITING);

    if running.is_some_and(|session| watched::read(&session.live, |live| live.closing)) {
        bail!("the session is closed; its attachments were removed");
    }

    let folder = folder().context("this host has no cache folder for attachments")?;
    let owner = owner(agent, session);
    write_in(&folder, &owner, path, offset, size, data)
}

fn owner(agent: &str, session: &SessionId) -> String {
    blake3::hash(format!("{agent}\0{session}").as_bytes())
        .to_hex()
        .to_string()
}

/// Deletes the upload bucket, including a file that is still being written.
pub(super) fn discard_draft(draft: &SessionId, path: &str) -> Result<()> {
    anyhow::ensure!(is_draft(DRAFT, draft), "invalid attachment draft");
    let cache = folder().context("this host has no cache folder for attachments")?;
    discard_in(&cache, &owner(DRAFT, draft), path)
}

fn discard_in(cache: &Path, owner: &str, path: &str) -> Result<()> {
    let _writing = lock(&WRITING);
    let cache = paths::canonicalize(cache)?;
    let (bucket, name) = upload_path(&cache, owner, path)?;

    match fs::symlink_metadata(&bucket) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => bail!("the attachment bucket is not a directory"),
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    }

    WorkRoot::at(&bucket)?;
    anyhow::ensure!(name.to_str() == Some(&record(&bucket)?.name), "the attachment name changed");
    fs::remove_dir_all(bucket).context("removing the draft attachment")
}

/// Only an explicit close or deletion removes completed attachments. Node
/// shutdown and agent failures leave them available for resume.
pub(super) fn remove(agent: &str, session: &SessionId) -> Result<()> {
    let Some(cache) = folder() else { return Ok(()) };
    remove_in(&cache, &owner(agent, session))
}

fn remove_in(cache: &Path, owner: &str) -> Result<()> {
    let _writing = lock(&WRITING);
    let path = cache.join(owner);

    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_dir() => {
            fs::remove_dir_all(path).context("removing session attachments")
        }
        Ok(_) => bail!("the session attachment folder is not a directory"),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn write_in(
    cache: &Path,
    owner: &str,
    path: &str,
    offset: u64,
    size: u64,
    data: &[u8],
) -> Result<String> {
    if data.len() > AGENT_FILE_CHUNK {
        bail!("the attachment part is too large");
    }

    let end = offset
        .checked_add(u64::try_from(data.len())?)
        .filter(|end| *end <= size)
        .context("a part runs past the attachment's size")?;
    let now = SystemTime::now();

    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(cache)?;
    let cache = paths::canonicalize(cache)?;
    let (bucket, upload) = if offset == 0 {
        file_name(path)?;

        let relative = Path::new(owner).join(Uuid::new_v4().to_string());
        WorkRoot::at(&cache)?.make_folder(&relative.join("files"))?;
        let bucket = cache.join(relative);
        let upload = Upload {
            size,
            name: path.to_owned(),
        };
        fs::write(bucket.join("upload.json"), serde_json::to_vec(&upload)?)?;
        (bucket, upload)
    } else {
        let (bucket, name) = upload_path(&cache, owner, path)?;
        WorkRoot::at(&bucket)?;
        let upload = record(&bucket)?;

        if name.to_str() != Some(&upload.name) || upload.size != size {
            bail!("the attachment changed during upload; drop it again");
        }
        (bucket, upload)
    };
    let root = WorkRoot::at(&bucket.join("files"))?;
    let written = work_folder::write(&root, &upload.name, offset, size, data)?;
    File::options()
        .write(true)
        .open(bucket.join("upload.json"))?
        .set_modified(now)?;
    let full = bucket.join("files").join(written);

    if end == size && !full.is_file() {
        bail!("the attachment did not finish; drop it again");
    }

    full.into_os_string()
        .into_string()
        .map_err(|_| anyhow::anyhow!("the attachment path is not UTF-8"))
}

fn upload_path<'a>(cache: &Path, owner: &str, path: &'a str) -> Result<(PathBuf, &'a std::ffi::OsStr)> {
    let group = cache.join(owner);
    let relative = Path::new(path)
        .strip_prefix(&group)
        .context("the attachment is outside this session's cache")?;
    let mut parts = relative.components();
    let (Some(Component::Normal(id)), Some(Component::Normal(files)), Some(Component::Normal(name)), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        bail!("the attachment path is invalid");
    };

    if files != "files" || Uuid::parse_str(&id.to_string_lossy()).is_err() {
        bail!("the attachment path is invalid");
    }

    Ok((group.join(id), name))
}

fn file_name(name: &str) -> Result<()> {
    let mut parts = Path::new(name).components();

    if name.is_empty()
        || name.contains(['/', '\\'])
        || !matches!(
            (parts.next(), parts.next()),
            (Some(Component::Normal(_)), None)
        )
    {
        bail!("an attachment needs a file name without folders");
    }

    Ok(())
}

fn record(bucket: &Path) -> Result<Upload> {
    let file = File::open(bucket.join("upload.json"))?;
    let upload: Upload = serde_json::from_reader(file.take(4096))?;
    file_name(&upload.name)?;
    Ok(upload)
}

/// Removes partial uploads abandoned for a day, a session's attachments a
/// day after it ended, and any attachment unused for three days. Directory
/// symlinks are never walked, and only folders with generated IDs belong to
/// this cache.
fn sweep(cache: &Path, now: SystemTime, running: &HashSet<String>) -> Result<()> {
    let groups = match fs::read_dir(cache) {
        Ok(groups) => groups,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };

    for group in groups {
        let group = group?;

        if !group.file_type()?.is_dir() {
            continue;
        }

        let ended = fs::symlink_metadata(group.path().join(ENDED))
            .is_ok_and(|meta| idle(now, &meta) >= DAY)
            && !running.contains(&*group.file_name().to_string_lossy());

        for bucket in fs::read_dir(group.path())? {
            let bucket = bucket?;

            if !bucket.file_type()?.is_dir()
                || Uuid::parse_str(&bucket.file_name().to_string_lossy()).is_err()
            {
                continue;
            }

            let path = bucket.path();
            let upload = record(&path).ok();
            let complete = upload
                .as_ref()
                .is_some_and(|upload| path.join("files").join(&upload.name).is_file());
            let unused = last_used(&path, now)?;

            if ended || unused >= UNUSED || (!complete && unused >= DAY) {
                fs::remove_dir_all(&path)?;
            }
        }

        if ended {
            fs::remove_file(group.path().join(ENDED))?;
        }

        // Empty session groups should not accumulate after expiry.
        match fs::remove_dir(group.path()) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::DirectoryNotEmpty => {}
            Err(e) => return Err(e.into()),
        }
    }

    Ok(())
}

/// How long since anything in an upload's folder was last read or written.
fn last_used(bucket: &Path, now: SystemTime) -> Result<Duration> {
    let mut least = idle(now, &fs::metadata(bucket.join("upload.json")).or_else(|_| bucket.metadata())?);

    if let Ok(files) = fs::read_dir(bucket.join("files")) {
        for file in files {
            least = least.min(idle(now, &file?.metadata()?));
        }
    }

    Ok(least)
}

#[cfg(test)]
#[path = "../tests/agents_uploads.rs"]
mod tests;
