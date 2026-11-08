//! Temporary attachments on this host, outside project checkouts.

use super::work_folder::{self, WorkRoot};
use crate::{files::paths, lock};
use anyhow::{Context, Result, bail};
use beep_proto::{AGENT_FILE_CHUNK, MAX_WORK_UPLOAD};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{ErrorKind, Read},
    path::{Component, Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime},
};
use tokio_util::task::AbortOnDropHandle;
use uuid::Uuid;

const DAY: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_BYTES: u64 = 5 * 1024 * 1024 * 1024;
const MAX_UPLOADS: usize = 1024;
// Reservations, chunks and cleanup must see one another's disk changes.
static WRITING: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize)]
struct Upload {
    size: u64,
    name: String,
}

pub(super) fn folder() -> Option<PathBuf> {
    dirs::cache_dir().map(|dir| dir.join("beep-node").join("attachments"))
}

/// Cleanup runs on startup and daily, and stops with the node.
pub(crate) fn maintain() -> AbortOnDropHandle<()> {
    AbortOnDropHandle::new(tokio::spawn(async {
        let mut every = tokio::time::interval(DAY);
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            every.tick().await;
            let done = tokio::task::spawn_blocking(|| -> Result<()> {
                let _writing = lock(&WRITING);
                if let Some(folder) = folder() {
                    sweep(&folder, SystemTime::now())?;
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

pub(super) fn write(
    agent: &str,
    session: &str,
    path: &str,
    offset: u64,
    size: u64,
    data: &[u8],
    running: Option<&super::Session>,
) -> Result<String> {
    let _writing = lock(&WRITING);
    if running.is_some_and(|session| session.live.borrow().closing) {
        bail!("the session is closed; its attachments were removed");
    }
    let folder = folder().context("this host has no cache folder for attachments")?;
    let owner = owner(agent, session);
    write_in(&folder, MAX_BYTES, &owner, path, offset, size, data)
}

fn owner(agent: &str, session: &str) -> String {
    blake3::hash(format!("{agent}\0{session}").as_bytes())
        .to_hex()
        .to_string()
}

/// Only an explicit close or deletion removes completed attachments. Node
/// shutdown and agent failures leave them available for resume.
pub(super) fn remove(agent: &str, session: &str) -> Result<()> {
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

#[allow(clippy::too_many_arguments)]
fn write_in(
    cache: &Path,
    limit: u64,
    owner: &str,
    path: &str,
    offset: u64,
    size: u64,
    data: &[u8],
) -> Result<String> {
    if size > MAX_WORK_UPLOAD || data.len() > AGENT_FILE_CHUNK {
        bail!("the attachment or its part is too large");
    }
    let end = offset
        .checked_add(u64::try_from(data.len())?)
        .filter(|end| *end <= size)
        .context("a part runs past the attachment's size")?;
    let now = SystemTime::now();
    // Each upload reserves its entire size, including parts not received yet.
    let (used, count) = sweep(cache, now)?;
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
        if count >= MAX_UPLOADS || used.checked_add(size).is_none_or(|sum| sum > limit) {
            bail!(
                "the temporary attachment cache is full; remove old attachments or upload into a project folder"
            );
        }
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
        let relative = Path::new(path)
            .strip_prefix(cache.join(owner))
            .context("the attachment is outside this session's cache")?;
        let parts: Vec<_> = relative.components().collect();
        let [
            Component::Normal(id),
            Component::Normal(files),
            Component::Normal(name),
        ] = parts.as_slice()
        else {
            bail!("the attachment path is invalid");
        };
        if *files != "files" || Uuid::parse_str(&id.to_string_lossy()).is_err() {
            bail!("the attachment path is invalid");
        }
        let bucket = cache.join(owner).join(id);
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

/// Counts reservations and removes expired uploads. Directory symlinks are
/// never walked, and only folders with generated IDs belong to this cache.
fn sweep(cache: &Path, now: SystemTime) -> Result<(u64, usize)> {
    let groups = match fs::read_dir(cache) {
        Ok(groups) => groups,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok((0, 0)),
        Err(e) => return Err(e.into()),
    };
    let mut used = 0u64;
    let mut count = 0usize;
    for group in groups {
        let group = group?;
        if !group.file_type()?.is_dir() {
            continue;
        }
        for bucket in fs::read_dir(group.path())? {
            let bucket = bucket?;
            if !bucket.file_type()?.is_dir()
                || Uuid::parse_str(&bucket.file_name().to_string_lossy()).is_err()
            {
                continue;
            }
            let path = bucket.path();
            let upload = record(&path).ok();
            let touched = fs::metadata(path.join("upload.json"))
                .or_else(|_| bucket.metadata())?
                .modified()?;
            let complete = upload
                .as_ref()
                .is_some_and(|upload| path.join("files").join(&upload.name).is_file());
            if !complete && now.duration_since(touched).unwrap_or_default() >= DAY {
                fs::remove_dir_all(&path)?;
            } else {
                used = used
                    .checked_add(upload.map_or(MAX_WORK_UPLOAD, |upload| upload.size))
                    .context("the attachment cache size overflowed")?;
                count += 1;
            }
        }
        // Empty session groups should not accumulate after expiry.
        match fs::remove_dir(group.path()) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::DirectoryNotEmpty => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok((used, count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn put(cache: &Path, session: &str, name: &str, bytes: &[u8]) -> String {
        write_in(
            cache,
            100,
            &owner("codex", session),
            name,
            0,
            bytes.len() as u64,
            bytes,
        )
        .unwrap()
    }

    #[test]
    fn uploads_are_outside_the_project_and_only_the_closed_sessions_files_are_removed() {
        let dir = TempDir::new().unwrap();
        let project = dir.path().join("project");
        let cache = dir.path().join("cache");
        fs::create_dir(&project).unwrap();
        fs::write(project.join("video.mp4"), b"permanent").unwrap();
        let first = put(&cache, "session-one", "video.mp4", b"one");
        let second = put(&cache, "session-two", "video.mp4", b"two");
        assert!(Path::new(&first).is_absolute());
        assert!(!Path::new(&first).starts_with(&project));
        assert_eq!(fs::read(&first).unwrap(), b"one");
        remove_in(&cache, &owner("codex", "session-one")).unwrap();
        assert!(!Path::new(&first).exists());
        assert_eq!(fs::read(&second).unwrap(), b"two");
        assert_eq!(fs::read(project.join("video.mp4")).unwrap(), b"permanent");
        assert!(!project.join(".beep").exists());
        remove_in(&cache, &owner("codex", "session-one")).unwrap();
    }

    #[test]
    fn parts_survive_a_restart_and_cannot_cross_sessions_or_change_size() {
        let dir = TempDir::new().unwrap();
        let cache = dir.path().join("cache");
        let session = owner("codex", "session-one");
        let path = write_in(&cache, 100, &session, "clip.mp4", 0, 6, b"one").unwrap();
        assert!(!Path::new(&path).exists());
        assert!(
            write_in(
                &cache,
                100,
                &owner("codex", "session-two"),
                &path,
                3,
                6,
                b"two"
            )
            .is_err()
        );
        assert!(write_in(&cache, 100, &session, &path, 3, 8, b"two").is_err());
        assert!(write_in(&cache, 100, &session, &path, 2, 6, b"two").is_err());
        // No process-local record is needed to finish after restarting.
        assert_eq!(
            write_in(&cache, 100, &session, &path, 3, 6, b"two").unwrap(),
            path
        );
        assert_eq!(fs::read(path).unwrap(), b"onetwo");
    }

    #[test]
    fn duplicate_names_do_not_overwrite_and_empty_files_are_uploaded() {
        let dir = TempDir::new().unwrap();
        let one = put(dir.path(), "session", "notes.txt", b"one");
        let two = put(dir.path(), "session", "notes.txt", b"two");
        assert_ne!(one, two);
        assert_eq!(fs::read(one).unwrap(), b"one");
        assert_eq!(fs::read(two).unwrap(), b"two");
        let empty = put(dir.path(), "session", "empty.txt", b"");
        assert_eq!(fs::read(empty).unwrap(), b"");
    }

    #[test]
    fn quota_reserves_the_whole_upload_and_close_reclaims_it() {
        let dir = TempDir::new().unwrap();
        let session = owner("codex", "session");
        let path = write_in(dir.path(), 10, &session, "clip.mp4", 0, 8, b"a").unwrap();
        assert!(write_in(dir.path(), 10, &session, "other.mp4", 0, 3, b"b").is_err());
        assert!(!Path::new(&path).exists());
        assert_eq!(
            write_in(dir.path(), 10, &session, &path, 1, 8, b"1234567").unwrap(),
            path
        );
        remove_in(dir.path(), &session).unwrap();
        assert!(write_in(dir.path(), 10, &session, "other.mp4", 0, 3, b"abc").is_ok());
    }

    #[test]
    fn only_abandoned_partial_uploads_expire() {
        let dir = TempDir::new().unwrap();
        let session = owner("codex", "session");
        let complete = put(dir.path(), "session", "keep.mp4", b"keep");
        let partial = write_in(dir.path(), 100, &session, "partial.mp4", 0, 8, b"part").unwrap();
        let partial_bucket = Path::new(&partial).parent().unwrap().parent().unwrap();
        let (used, count) = sweep(dir.path(), SystemTime::now() + DAY * 60).unwrap();
        assert!(Path::new(&complete).exists());
        assert!(!partial_bucket.exists());
        assert_eq!((used, count), (4, 1));
    }

    #[test]
    fn names_and_oversized_uploads_cannot_escape_the_cache() {
        let dir = TempDir::new().unwrap();
        let session = owner("codex", "session");
        for name in ["../clip.mp4", "/clip.mp4", "a/b", "a\\b", "", ".", ".."] {
            assert!(
                write_in(dir.path(), 100, &session, name, 0, 1, b"a").is_err(),
                "{name:?}"
            );
        }
        assert!(
            write_in(
                dir.path(),
                MAX_BYTES,
                &session,
                "huge.mp4",
                0,
                MAX_WORK_UPLOAD + 1,
                b"a"
            )
            .is_err()
        );
        assert!(write_in(dir.path(), 100, &session, "bad.mp4", u64::MAX, 1, b"a").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_session_folders_are_never_written_or_cleaned() {
        let dir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let session = owner("codex", "session");
        fs::write(outside.path().join("keep.txt"), b"keep").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join(&session)).unwrap();
        assert!(write_in(dir.path(), 100, &session, "clip.mp4", 0, 1, b"a").is_err());
        assert!(remove_in(dir.path(), &session).is_err());
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 1);
    }
}
