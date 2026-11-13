use super::*;
use tempfile::TempDir;

fn put(cache: &Path, session: &str, name: &str, bytes: &[u8]) -> String {
    write_in(
        cache,
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
    let path = write_in(&cache, &session, "clip.mp4", 0, 6, b"one").unwrap();
    assert!(!Path::new(&path).exists());
    assert!(write_in(&cache, &owner("codex", "session-two"), &path, 3, 6, b"two").is_err());
    assert!(write_in(&cache, &session, &path, 3, 8, b"two").is_err());
    assert!(write_in(&cache, &session, &path, 2, 6, b"two").is_err());
    // No process-local record is needed to finish after restarting.
    assert_eq!(
        write_in(&cache, &session, &path, 3, 6, b"two").unwrap(),
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
fn attachments_have_no_file_or_aggregate_size_quota() {
    let dir = TempDir::new().unwrap();
    let session = owner("codex", "session");
    let size = 6 * 1024 * 1024 * 1024;
    let first = write_in(dir.path(), &session, "large.mp4", 0, size, b"a").unwrap();
    let part = Path::new(&first)
        .parent()
        .unwrap()
        .join(".large.mp4.beep-part");
    File::options()
        .write(true)
        .open(part)
        .unwrap()
        .set_len(size - 1)
        .unwrap();
    assert_eq!(
        write_in(dir.path(), &session, &first, size - 1, size, b"z").unwrap(),
        first
    );
    assert_eq!(fs::metadata(&first).unwrap().len(), size);
    assert!(write_in(dir.path(), &session, "other.mp4", 0, size, b"a").is_ok());
    sweep(dir.path(), SystemTime::now() + DAY * 60).unwrap();
    assert_eq!(fs::metadata(&first).unwrap().len(), size);
}

#[test]
fn only_abandoned_partial_uploads_expire() {
    let dir = TempDir::new().unwrap();
    let session = owner("codex", "session");
    let complete = put(dir.path(), "session", "keep.mp4", b"keep");
    let partial = write_in(dir.path(), &session, "partial.mp4", 0, 8, b"part").unwrap();
    let partial_bucket = Path::new(&partial).parent().unwrap().parent().unwrap();
    sweep(dir.path(), SystemTime::now() + DAY * 60).unwrap();
    assert!(Path::new(&complete).exists());
    assert!(!partial_bucket.exists());
}

#[test]
fn invalid_names_and_chunks_cannot_escape_the_cache() {
    let dir = TempDir::new().unwrap();
    let session = owner("codex", "session");

    for name in ["../clip.mp4", "/clip.mp4", "a/b", "a\\b", "", ".", ".."] {
        assert!(
            write_in(dir.path(), &session, name, 0, 1, b"a").is_err(),
            "{name:?}"
        );
    }

    assert!(
        write_in(
            dir.path(),
            &session,
            "huge.mp4",
            0,
            1,
            &vec![0; AGENT_FILE_CHUNK + 1]
        )
        .is_err()
    );
    assert!(write_in(dir.path(), &session, "bad.mp4", u64::MAX, 1, b"a").is_err());
}

#[cfg(unix)]
#[test]
fn symlinked_session_folders_are_never_written_or_cleaned() {
    let dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let session = owner("codex", "session");
    fs::write(outside.path().join("keep.txt"), b"keep").unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join(&session)).unwrap();
    assert!(write_in(dir.path(), &session, "clip.mp4", 0, 1, b"a").is_err());
    assert!(remove_in(dir.path(), &session).is_err());
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 1);
}
