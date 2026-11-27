use super::*;
use crate::agents::{Agents, work_folder::prune};
use super::maintain::of;
use tempfile::TempDir;

fn put(cache: &Path, session: &str, name: &str, bytes: &[u8]) -> String {
    write_in(
        cache,
        &owner("codex", &SessionId::from(session)),
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
    remove_in(&cache, &owner("codex", &SessionId::from("session-one"))).unwrap();
    assert!(!Path::new(&first).exists());
    assert_eq!(fs::read(&second).unwrap(), b"two");
    assert_eq!(fs::read(project.join("video.mp4")).unwrap(), b"permanent");
    assert!(!project.join(".beep").exists());
    remove_in(&cache, &owner("codex", &SessionId::from("session-one"))).unwrap();
}

#[test]
fn parts_survive_a_restart_and_cannot_cross_sessions_or_change_size() {
    let dir = TempDir::new().unwrap();
    let cache = dir.path().join("cache");
    let session = owner("codex", &SessionId::from("session-one"));
    let path = write_in(&cache, &session, "clip.mp4", 0, 6, b"one").unwrap();
    assert!(!Path::new(&path).exists());
    assert!(write_in(&cache, &owner("codex", &SessionId::from("session-two")), &path, 3, 6, b"two").is_err());
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
fn discarding_a_draft_upload_removes_its_parts_only() {
    let dir = TempDir::new().unwrap();
    let draft = SessionId::from(Uuid::new_v4().to_string());
    assert!(is_draft(DRAFT, &draft));
    assert!(!is_draft(DRAFT, &SessionId::from("session")));
    let draft = owner(DRAFT, &draft);
    let cache = dir.path();
    let partial = write_in(cache, &draft, "notes.txt", 0, 8, b"part").unwrap();
    let complete = write_in(cache, &draft, "notes.txt", 0, 4, b"keep").unwrap();
    let other = put(cache, "session", "notes.txt", b"also keep");

    assert!(discard_in(cache, &draft, &other).is_err());
    discard_in(cache, &draft, &partial).unwrap();
    discard_in(cache, &draft, &partial).unwrap();

    assert!(write_in(cache, &draft, &partial, 4, 8, b"rest").is_err());
    assert_eq!(fs::read(complete).unwrap(), b"keep");
    assert_eq!(fs::read(other).unwrap(), b"also keep");
}

#[test]
fn attachments_have_no_file_or_aggregate_size_quota() {
    let dir = TempDir::new().unwrap();
    let session = owner("codex", &SessionId::from("session"));
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
    sweep(dir.path(), SystemTime::now() + DAY / 2, &HashSet::new()).unwrap();
    assert_eq!(fs::metadata(&first).unwrap().len(), size);
}

#[test]
fn only_abandoned_partial_uploads_expire() {
    let dir = TempDir::new().unwrap();
    let session = owner("codex", &SessionId::from("session"));
    let complete = put(dir.path(), "session", "keep.mp4", b"keep");
    let partial = write_in(dir.path(), &session, "partial.mp4", 0, 8, b"part").unwrap();
    let partial_bucket = Path::new(&partial).parent().unwrap().parent().unwrap();
    sweep(dir.path(), SystemTime::now() + DAY * 2, &HashSet::new()).unwrap();
    assert!(Path::new(&complete).exists());
    assert!(!partial_bucket.exists());
}

#[test]
fn attachments_go_a_day_after_their_session_ends_unless_it_runs_again() {
    let dir = TempDir::new().unwrap();
    let ended = put(dir.path(), "ended", "a.txt", b"a");
    let resumed = put(dir.path(), "resumed", "b.txt", b"b");
    let open = put(dir.path(), "open", "c.txt", b"c");

    for session in ["ended", "resumed"] {
        fs::write(dir.path().join(owner("codex", &SessionId::from(session))).join(ENDED), b"").unwrap();
    }

    let running = HashSet::from([owner("codex", &SessionId::from("resumed"))]);
    sweep(dir.path(), SystemTime::now() + DAY / 2, &running).unwrap();
    assert!(Path::new(&ended).exists(), "within a day of the end");
    sweep(dir.path(), SystemTime::now() + DAY * 2, &running).unwrap();
    assert!(!dir.path().join(owner("codex", &SessionId::from("ended"))).exists());
    assert!(Path::new(&resumed).exists());
    assert!(Path::new(&open).exists());
}

#[test]
fn attachments_unused_for_three_days_go_even_while_their_session_runs() {
    let dir = TempDir::new().unwrap();
    let file = put(dir.path(), "open", "a.txt", b"a");
    let running = HashSet::from([owner("codex", &SessionId::from("open"))]);
    sweep(dir.path(), SystemTime::now() + DAY * 2, &running).unwrap();
    assert!(Path::new(&file).exists());
    sweep(dir.path(), SystemTime::now() + UNUSED + DAY, &running).unwrap();
    assert!(!Path::new(&file).exists());
}

#[test]
fn project_uploads_unused_for_three_days_go() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("clip.mp4"), b"clip").unwrap();
    fs::create_dir(dir.path().join("folder")).unwrap();
    prune(dir.path(), SystemTime::now() + DAY * 2).unwrap();
    assert!(dir.path().join("clip.mp4").exists());
    prune(dir.path(), SystemTime::now() + UNUSED + DAY).unwrap();
    assert!(!dir.path().join("clip.mp4").exists());
    assert!(dir.path().join("folder").exists());
}

#[test]
fn invalid_names_and_chunks_cannot_escape_the_cache() {
    let dir = TempDir::new().unwrap();
    let session = owner("codex", &SessionId::from("session"));

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
    let session = owner("codex", &SessionId::from("session"));
    fs::write(outside.path().join("keep.txt"), b"keep").unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join(&session)).unwrap();
    assert!(write_in(dir.path(), &session, "clip.mp4", 0, 1, b"a").is_err());
    assert!(remove_in(dir.path(), &session).is_err());
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn the_sweep_cleans_the_folder_a_session_was_refiled_to() {
    use crate::agents::{AgentKind, session::Begin};
    use std::collections::BTreeMap;

    const ADAPTER: &str = r#"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
read -r line
reply '{"protocolVersion":1,"agentCapabilities":{}}'
while read -r line; do
  case "$line" in
    *'"session/new"'*) reply '{"sessionId":"refiled-1"}' ;;
    *'"id"'*) reply '{}' ;;
  esac
done
"#;
    let dir = tempfile::tempdir().unwrap();
    let kind = AgentKind {
        name: "fake".into(),
        program: "sh".into(),
        args: vec!["-c".into(), ADAPTER.into()],
        env: BTreeMap::new(),
        compact: None,
        compact_models: Vec::new(),
        default_model: None,
        bridge: None,
    };
    let agents = Agents::new(vec![kind.clone()], crate::occupancy::Occupancy::default());
    let session = agents
        .start(&kind, Some(&dir.path().to_string_lossy()), Begin::default())
        .await
        .unwrap();
    session.named_info().await.unwrap();
    session.close().await;
    session.moved_to("/srv/refiled");

    let (_, folders) = of(&agents.session_map());

    assert!(folders.contains(Path::new("/srv/refiled")));
    assert!(!folders.contains(dir.path()));
    agents.shutdown().await;
}
