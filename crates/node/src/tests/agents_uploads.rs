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
