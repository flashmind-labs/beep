use super::*;
use iroh::SecretKey;
use tempfile::TempDir;

fn entry(id: &str, activity: u64, bytes: u64) -> Entry {
    Entry {
        agent: AgentSlug::CODEX,
        id: id.to_owned(),
        copy: Some(Meta {
            from: SecretKey::generate().public(),
            project: "p".into(),
            path: String::new(),
            file_name: "f.jsonl".into(),
            size: bytes,
            hash: Vec::new(),
            last_activity: activity,
            fetched_at: 0,
        }),
        lost: Vec::new(),
        copy_bytes: bytes,
        lost_bytes: 0,
    }
}

fn ids(gone: Vec<(AgentSlug, String)>) -> Vec<String> {
    gone.into_iter().map(|(_, id)| id).collect()
}

#[test]
fn the_oldest_copies_make_room_for_a_newer_session() {
    let held = [
        entry("a", 30, 400),
        entry("b", 10, 400),
        entry("c", 20, 400),
    ];
    assert_eq!(
        ids(evictions(&held, 1500, 100, 40, None).unwrap()),
        Vec::<String>::new()
    );
    assert_eq!(ids(evictions(&held, 1500, 400, 40, None).unwrap()), ["b"]);
    assert_eq!(
        ids(evictions(&held, 1000, 600, 40, None).unwrap()),
        ["b", "c"]
    );
}

#[test]
fn a_session_older_than_what_is_held_never_pushes_it_out() {
    let held = [entry("a", 30, 400), entry("b", 20, 400)];
    assert_eq!(evictions(&held, 900, 400, 10, None), None);
    // Nor does one that cannot fit even in an empty cache.
    assert_eq!(evictions(&held, 300, 400, 99, None), None);
}

#[test]
fn a_version_that_lost_is_never_evicted_and_counts_against_the_cap() {
    let mut kept = entry("a", 10, 100);
    kept.copy = None;
    kept.lost = vec![5];
    kept.copy_bytes = 0;
    kept.lost_bytes = 800;
    let held = [kept, entry("b", 10, 100)];
    assert_eq!(ids(evictions(&held, 1000, 150, 50, None).unwrap()), ["b"]);
    assert_eq!(
        evictions(&held, 950, 100, 50, None).map(ids),
        Some(vec!["b".to_owned()])
    );
    assert_eq!(evictions(&held, 850, 100, 50, None), None);
}

#[test]
fn a_continued_note_and_a_lost_version_are_kept_by_their_names() {
    let dir = TempDir::new().expect("a temp folder");
    let cache = Cache::new(dir.path());
    let agent = AgentSlug::CODEX;
    assert_eq!(cache.continued(&agent, "s1"), None);
    cache.mark_continued(&agent, "s1", 42, 50).expect("noted");
    assert_eq!(
        cache.continued(&agent, "s1"),
        Some(Continued { base: 42, at: 50 })
    );
    let transcript = dir.path().join("t.jsonl");
    fs::write(&transcript, b"{}\n").expect("a transcript");
    let meta = entry("s1", 7, 3).copy.expect("meta");
    cache
        .keep_lost(&agent, "s1", &transcript, &meta, 9)
        .expect("kept");
    assert!(!transcript.exists());
    let found = cache.entries();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].lost, [9]);
    assert!(cache.copy(&agent, "s1").is_some());
    let (path, _) = cache.lost(&agent, "s1", 9).expect("the version is kept");
    assert_eq!(fs::read(&path).unwrap(), b"{}\n");
    #[cfg(unix)]
    assert_eq!(
        std::os::unix::fs::PermissionsExt::mode(&fs::metadata(&path).unwrap().permissions())
            & 0o777,
        0o600
    );
    cache.drop_lost(&agent, "s1", 9);
    assert!(cache.entries()[0].lost.is_empty());
    assert!(cache.copy(&AgentSlug::from("../x"), "s1").is_none());
    assert!(cache.copy(&agent, "../s1").is_none());
}

#[test]
fn two_versions_set_aside_in_the_same_second_both_stay() {
    let dir = TempDir::new().expect("a temp folder");
    let cache = Cache::new(dir.path());
    let agent = AgentSlug::CODEX;
    let meta = entry("s1", 7, 3).copy.expect("meta");
    for (name, text) in [("one.jsonl", "1\n"), ("two.jsonl", "2\n")] {
        let transcript = dir.path().join(name);
        fs::write(&transcript, text).expect("a transcript");
        cache
            .keep_lost(&agent, "s1", &transcript, &meta, 9)
            .expect("kept");
    }
    let lost = cache.entries()[0].lost.clone();
    assert_eq!(lost.len(), 2, "{lost:?}");
    let texts: Vec<_> = lost
        .iter()
        .map(|at| fs::read_to_string(cache.lost(&agent, "s1", *at).unwrap().0).unwrap())
        .collect();
    assert!(texts.contains(&"1\n".to_owned()) && texts.contains(&"2\n".to_owned()));
}

#[test]
fn a_version_set_aside_goes_back_only_where_nothing_is() {
    let dir = TempDir::new().expect("a temp folder");
    let cache = Cache::new(dir.path());
    let agent = AgentSlug::CODEX;
    let meta = entry("s1", 7, 3).copy.expect("meta");
    let store = dir.path().join("store.jsonl");
    fs::write(&store, "mine\n").expect("a transcript");
    let at = cache
        .keep_lost(&agent, "s1", &store, &meta, 9)
        .expect("kept");
    assert!(!store.exists());
    // The session did not start: it goes back, and is no longer listed.
    assert!(cache.put_back(&agent, "s1", at, &store));
    assert_eq!(fs::read_to_string(&store).unwrap(), "mine\n");
    assert!(cache.entries().iter().all(|e| e.lost.is_empty()));
    // Something else is there now: the version stays listed, untouched.
    let at = cache
        .keep_lost(&agent, "s1", &store, &meta, 9)
        .expect("kept again");
    fs::write(&store, "written meanwhile\n").expect("another transcript");
    assert!(!cache.put_back(&agent, "s1", at, &store));
    assert_eq!(cache.entries()[0].lost, [at]);
    assert_eq!(fs::read_to_string(&store).unwrap(), "written meanwhile\n");
}
