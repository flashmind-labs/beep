use super::*;
use std::{collections::VecDeque, future::ready, sync::Arc};
use tempfile::TempDir;
use tokio::sync::Notify;

fn chunk(size: u64, data: &[u8]) -> AgentFileChunk {
    AgentFileChunk {
        size,
        mime: "application/octet-stream".into(),
        data: data.to_vec(),
    }
}

#[tokio::test]
async fn streamed_downloads_use_contiguous_offsets_and_keep_the_bytes() {
    let dir = TempDir::new().unwrap();
    let mut chunks = VecDeque::from([chunk(6, b"abc"), chunk(6, b"def")]);
    let mut offsets = Vec::new();
    let temporary = download_file(dir.path(), |offset| {
        offsets.push(offset);
        ready(Ok(chunks.pop_front().unwrap()))
    })
    .await
    .unwrap();

    assert_eq!(offsets, [0, 3]);
    assert_eq!(tokio::fs::read(&temporary).await.unwrap(), b"abcdef");
    let to = dir.path().join("complete.bin");
    temporary.persist_noclobber(&to).unwrap();
    assert_eq!(tokio::fs::read(to).await.unwrap(), b"abcdef");
}

#[tokio::test]
async fn an_empty_remote_file_creates_an_empty_download() {
    let dir = TempDir::new().unwrap();
    let temporary = download_file(dir.path(), |offset| {
        assert_eq!(offset, 0);
        ready(Ok(chunk(0, b"")))
    })
    .await
    .unwrap();

    assert!(tokio::fs::read(temporary).await.unwrap().is_empty());
}

#[tokio::test]
async fn invalid_chunks_leave_no_partial_file_or_changed_destination() {
    let dir = TempDir::new().unwrap();
    let to = dir.path().join("saved.bin");
    tokio::fs::write(&to, b"keep").await.unwrap();
    let oversized = AgentFileChunk {
        size: (AGENT_FILE_CHUNK + 1) as u64,
        mime: "application/octet-stream".into(),
        data: vec![0; AGENT_FILE_CHUNK + 1],
    };

    for chunks in [
        vec![chunk(2, b"a"), chunk(2, b"")],
        vec![chunk(3, b"a"), chunk(2, b"b")],
        vec![chunk(1, b"ab")],
        vec![oversized],
    ] {
        let mut chunks = VecDeque::from(chunks);
        let result = download_file(dir.path(), |_| ready(Ok(chunks.pop_front().unwrap()))).await;
        assert!(matches!(result, Err(DesktopError::BadRequest(_))));
        assert_eq!(tokio::fs::read(&to).await.unwrap(), b"keep");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

#[tokio::test]
async fn read_errors_preserve_the_error_and_remove_the_temporary_file() {
    let dir = TempDir::new().unwrap();
    let mut first = true;
    let result = download_file(dir.path(), |_| {
        ready(if std::mem::take(&mut first) {
            Ok(chunk(2, b"a"))
        } else {
            Err(DesktopError::Lost("build-box".into()))
        })
    })
    .await;

    assert!(matches!(result, Err(DesktopError::Lost(name)) if name == "build-box"));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn a_save_race_keeps_the_file_that_appeared_during_the_download() {
    let dir = TempDir::new().unwrap();
    let to = dir.path().join("saved.bin");
    let temporary = download_file(dir.path(), |_| ready(Ok(chunk(3, b"new"))))
        .await
        .unwrap();
    tokio::fs::write(&to, b"keep").await.unwrap();

    assert!(temporary.persist_noclobber(&to).is_err());
    assert_eq!(tokio::fs::read(to).await.unwrap(), b"keep");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn cancellation_removes_an_unfinished_download() {
    let dir = TempDir::new().unwrap();
    let waiting = Arc::new(Notify::new());
    let task = tokio::spawn({
        let dir = dir.path().to_owned();
        let waiting = waiting.clone();
        async move {
            let mut first = true;
            download_file(&dir, |_| {
                let first = std::mem::take(&mut first);
                let waiting = waiting.clone();
                async move {
                    if first {
                        Ok(chunk(2, b"a"))
                    } else {
                        waiting.notify_one();
                        std::future::pending().await
                    }
                }
            })
            .await
        }
    });
    waiting.notified().await;
    task.abort();

    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

fn open_copy(dir: &Path, id: u64, size: u64, age: Duration, now: SystemTime) -> PathBuf {
    let path = dir.join(format!("{id:016x}-copy.bin"));
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(size).unwrap();
    file.set_modified(now.checked_sub(age).unwrap()).unwrap();
    path
}

#[tokio::test]
async fn native_copies_expire_and_the_oldest_go_over_budget() {
    let dir = TempDir::new().unwrap();
    let now = SystemTime::UNIX_EPOCH + KEEP * 10;
    let expired = open_copy(dir.path(), 1, 3, KEEP + Duration::from_secs(1), now);
    let oldest = open_copy(dir.path(), 2, 3, Duration::from_secs(3), now);
    let middle = open_copy(dir.path(), 3, 3, Duration::from_secs(2), now);
    let newest = open_copy(dir.path(), 4, 3, Duration::from_secs(1), now);

    prune_open_files(dir.path(), None, now, 6).await.unwrap();

    assert!(!expired.exists());
    assert!(!oldest.exists());
    assert!(middle.exists());
    assert!(newest.exists());
}

#[tokio::test]
async fn the_copy_being_opened_stays_while_other_copies_are_pruned() {
    let dir = TempDir::new().unwrap();
    let now = SystemTime::UNIX_EPOCH + KEEP * 10;
    let current = open_copy(dir.path(), 1, 9, Duration::from_secs(2), now);
    let other = open_copy(dir.path(), 2, 1, Duration::from_secs(1), now);

    prune_open_files(dir.path(), Some(&current), now, 6)
        .await
        .unwrap();

    assert!(current.exists());
    assert!(!other.exists());
}

#[tokio::test]
async fn startup_sweeps_keep_the_latest_copy_until_it_ages_out() {
    let dir = TempDir::new().unwrap();
    let now = SystemTime::UNIX_EPOCH + KEEP * 10;
    let current = open_copy(dir.path(), 1, 9, Duration::ZERO, now);

    prune_open_files(dir.path(), None, now, 6).await.unwrap();
    assert!(current.exists());

    prune_open_files(dir.path(), None, now + KEEP + Duration::from_secs(1), 6)
        .await
        .unwrap();
    assert!(!current.exists());
}

#[tokio::test]
async fn sweeps_touch_only_completed_media_copies() {
    let dir = TempDir::new().unwrap();
    let now = SystemTime::UNIX_EPOCH + KEEP * 10;
    let owned = open_copy(dir.path(), 1, 3, KEEP + Duration::from_secs(1), now);
    let unrelated = dir.path().join("keep.bin");
    let partial = NamedTempFile::new_in(dir.path()).unwrap();
    let directory = dir.path().join("0000000000000002-folder");
    std::fs::write(&unrelated, b"keep").unwrap();
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join("keep.bin"), b"keep").unwrap();

    prune_open_files(dir.path(), None, now, 0).await.unwrap();

    assert!(!owned.exists());
    assert_eq!(std::fs::read(unrelated).unwrap(), b"keep");
    assert!(partial.path().exists());
    assert!(directory.join("keep.bin").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn sweeps_never_follow_file_or_directory_symlinks() {
    let dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let now = SystemTime::UNIX_EPOCH + KEEP * 10;
    let foreign = open_copy(outside.path(), 1, 3, KEEP + Duration::from_secs(1), now);
    let linked_file = dir.path().join("0000000000000002-linked.bin");
    std::os::unix::fs::symlink(&foreign, &linked_file).unwrap();

    prune_open_files(dir.path(), None, now, 0).await.unwrap();
    assert!(foreign.exists());
    assert!(
        std::fs::symlink_metadata(&linked_file)
            .unwrap()
            .file_type()
            .is_symlink()
    );

    let linked_dir = dir.path().join("linked-directory");
    std::os::unix::fs::symlink(outside.path(), &linked_dir).unwrap();
    prune_open_files(&linked_dir, None, now, 0).await.unwrap();
    assert!(foreign.exists());
}
