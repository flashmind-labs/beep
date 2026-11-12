use super::*;
use iroh::SecretKey;
use tempfile::TempDir;

fn offer(id: &str, activity: u64, size: u64) -> Offer {
    Offer {
        agent: AgentSlug::CODEX,
        id: id.to_owned(),
        project: "p".into(),
        path: String::new(),
        last_activity: activity,
        size,
        hash: Vec::new(),
    }
}

/// A cache holding a copy of `bytes` bytes of each session in `held`, last
/// active at its time.
fn cache_with(dir: &Path, held: &[(&str, u64)], bytes: usize) -> Cache {
    let cache = Cache::new(dir);
    let from = SecretKey::generate().public();

    for (id, activity) in held {
        let agent = AgentSlug::CODEX;
        let copy = cache.copy(&agent, id).expect("a session name");
        fs::create_dir_all(copy.parent().expect("a folder")).expect("the folder is made");
        fs::write(&copy, vec![b'x'; bytes]).expect("a copy");
        let meta = Meta {
            from,
            project: "p".into(),
            path: String::new(),
            file_name: "f.jsonl".into(),
            size: bytes as u64,
            hash: Vec::new(),
            last_activity: *activity,
            fetched_at: 0,
        };
        cache.save_meta(&agent, id, &meta).expect("the note is saved");
    }

    cache
}

#[test]
fn a_full_cache_is_read_once_and_ends_the_pass() {
    let dir = TempDir::new().expect("a temp folder");
    let cache = cache_with(dir.path(), &[("a", 30), ("b", 20), ("c", 10)], 1000);
    let entries = cache.entries();
    let total: u64 = entries.iter().map(|e| e.copy_bytes).sum();
    // Full: not even a note fits.
    let mut room = Room::new(entries, total + NOTE_ROOM - 1);
    // What the pass decides comes from that one read: the folder can go.
    fs::remove_dir_all(dir.path().join("sessions")).expect("the cache is removed");

    // Too big for the cache alone: the pass goes on with smaller ones.
    assert_eq!(room.make_room(&offer("huge", 99, 1 << 20)), Err(CacheFull));
    assert!(!room.full);
    // A newer one makes room by taking the oldest away.
    let gone = room.make_room(&offer("new", 50, 1000)).expect("room is made");
    assert_eq!(gone, [(AgentSlug::CODEX, "c".to_owned())]);
    // The pull lands in the room the oldest left.
    let mut new = room.get(&AgentSlug::CODEX, "c").expect("c is held").clone();
    new.id = "new".into();
    new.copy.as_mut().expect("a note").last_activity = 50;
    room.replace(&gone, vec![new]);

    // An older one than every copy left finds no room, and no session
    // after it, newest first, can.
    assert_eq!(room.make_room(&offer("old", 15, 1000)), Err(CacheFull));
    assert!(room.full);

    for n in 0..170 {
        assert_eq!(
            room.make_room(&offer(&format!("s{n}"), 1, 10)),
            Err(CacheFull)
        );
    }
}

#[test]
fn a_stalled_pass_is_skipped_until_what_it_saw_changes() {
    let dir = TempDir::new().expect("a temp folder");
    let cache = cache_with(dir.path(), &[("a", 30)], 10);
    let seen = Seen {
        offers: 7,
        changes: cache.changes(),
        cap: 100,
    };
    let at = Instant::now();
    let stalled = Stalled { seen, at };
    assert!(stalled.skips(&seen, at + Duration::from_secs(60)));
    assert!(!stalled.skips(&seen, at + RECHECK));
    assert!(!stalled.skips(&Seen { offers: 8, ..seen }, at));
    assert!(!stalled.skips(&Seen { cap: 200, ..seen }, at));

    // Any change through any clone of the cache counts.
    cache.clone().remove(&AgentSlug::CODEX, "a", false);
    let changes = cache.changes();
    assert!(!stalled.skips(&Seen { changes, ..seen }, at));
}
