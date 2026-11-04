use super::{
    fetch::{copy_out, copy_store, fetch_within, peers_to_ask},
    list::{Merged, merge_peer, move_target, plan, record},
    scanning::has_baseline,
    shared::{Shared, Start},
    state::{Saved, Seen, State},
    task::{DELETE_SETTLE, DeleteTimer, Task},
};
use crate::files::sync::{
    Parts,
    map::{Entry, FileMeta, Map, Order, Version, is_conflict},
    rules::{MARKER, Rules},
    scan::{self, Found, MAX_FILES, Stat},
    watch::{Mode, Watcher},
};
use crate::{
    files::Folders,
    files::transfer::place::{Journal, open_target},
};
use anyhow::Result;
use beep_proto::node::sync::{Keep, State as SyncState};
use futures::{FutureExt as _, StreamExt as _};
use iroh::endpoint::Connection;
use iroh::{Endpoint, EndpointAddr, SecretKey, endpoint::presets};
use iroh_blobs::store::fs::FsStore;
use iroh_blobs::{Hash, HashAndFormat};
use std::{
    collections::HashMap,
    fs,
    io::{SeekFrom, Write as _},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tempfile::TempDir;
use tokio::{
    fs::File as AsyncFile,
    io::{AsyncSeekExt, AsyncWriteExt},
    time::{Instant, timeout},
};
use tokio_util::sync::CancellationToken;

/// A project task over a folder of files, scanned once, driven by
/// hand: no watcher wakes it and no timer runs.
struct Project {
    _dir: TempDir,
    base: PathBuf,
    folder: PathBuf,
    task: Task,
}

impl Project {
    async fn new(files: &[&str]) -> Self {
        let dir = TempDir::new().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let folder = base.join("game");
        for file in files {
            let path = folder.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, file).unwrap();
        }
        let folders = Folders::new(std::slice::from_ref(&base));
        let root = open_target(&folders, &folder.to_string_lossy()).unwrap();
        let parts = Parts {
            endpoint: Endpoint::builder(presets::Minimal).bind().await.unwrap(),
            store: FsStore::load(base.join("store")).await.unwrap(),
            store_dir: base.join("store"),
            journal: Journal::open(base.join("partial")).unwrap(),
            room: Arc::default(),
        };
        let shared = Shared::load(Start {
            name: "game".into(),
            key: 1,
            root: Arc::new(root),
            peers: Vec::new(),
            file: base.join("state"),
            paused: false,
        })
        .unwrap();
        let watcher = Watcher::with_budget(None, None);
        let mut task = Task::new(shared, parts, watcher, Mode::Watching);
        task.scan().await;
        assert_eq!(task.error, None);
        Self {
            _dir: dir,
            base,
            folder,
            task,
        }
    }

    fn path(&self, path: &str) -> PathBuf {
        self.folder.join(path)
    }

    /// The paths the list holds as deleted: what peers would delete.
    fn deleted(&self) -> Vec<String> {
        self.task
            .shared
            .lock()
            .saved
            .map
            .entries()
            .filter(|entry| entry.file.is_none())
            .map(|entry| entry.path.clone())
            .collect()
    }

    fn held(&self) -> Vec<&str> {
        self.task.held.iter().map(String::as_str).collect()
    }

    async fn stop(self) {
        self.task.parts.store.shutdown().await.ok();
        self.task.parts.endpoint.close().await;
    }
}

const COPY: &str = "docs/notes (conflict 1a2b3c4d).txt";

#[tokio::test]
async fn keeping_the_copy_puts_it_in_place_of_the_file() {
    let mut project = Project::new(&["docs/notes.txt", COPY]).await;
    project.task.shared.resolve(COPY, Keep::Copy).await.unwrap();
    assert_eq!(
        fs::read_to_string(project.path("docs/notes.txt")).unwrap(),
        COPY
    );
    assert!(!project.path(COPY).exists());
    // The scan records the new content, and holds the copy's delete.
    project.task.scan().await;
    assert_eq!(project.held(), [COPY]);
    let listed = project
        .task
        .shared
        .lock()
        .saved
        .map
        .get("docs/notes.txt")
        .and_then(|entry| entry.file.as_ref().map(|file| file.hash));
    assert_eq!(listed, Some(*Hash::new(COPY).as_bytes()));
    project.stop().await;
}

/// A blob peer that takes connections and never answers on them, like
/// one whose path stopped carrying data while the connection lives.
#[derive(Debug, Clone)]
struct Silent;

impl iroh::protocol::ProtocolHandler for Silent {
    async fn accept(&self, connection: Connection) -> Result<(), iroh::protocol::AcceptError> {
        connection.closed().await;
        Ok(())
    }
}

#[test]
fn a_peer_that_is_off_does_not_hold_up_every_file() {
    let addr = |seed: u8| {
        let id = SecretKey::from_bytes(&[seed; 32]).public();
        EndpointAddr::new(id)
    };
    let (off, on) = (addr(1), addr(2));
    let peers = [off.clone(), on.clone()];
    let ids = |ask: Vec<EndpointAddr>| ask.iter().map(|peer| peer.id).collect::<Vec<_>>();
    let now = Instant::now() + Duration::from_secs(3600);

    let nobody_down = HashMap::new();
    assert_eq!(
        ids(peers_to_ask(&peers, &nobody_down, now)),
        [off.id, on.id]
    );

    // Just failed: not dialled again while another file waits.
    let down = HashMap::from([(off.id, now)]);
    assert_eq!(ids(peers_to_ask(&peers, &down, now)), [on.id]);

    // Worth another try, once the peers that answer had their turn.
    let later = now + Duration::from_secs(30);
    assert_eq!(ids(peers_to_ask(&peers, &down, later)), [on.id, off.id]);
}

/// What the store's collector keeps, apart from named tags.
async fn temp_roots(project: &Project) -> Vec<Hash> {
    let tags = project.task.parts.store.tags();
    let roots = tags.list_temp_tags().await.unwrap();
    roots.map(|root| root.hash).collect().await
}

#[tokio::test]
async fn a_failed_fetch_keeps_what_came_until_the_file_is_not_wanted() {
    let mut project = Project::new(&["notes.txt"]).await;
    let mut peer = Map::new(7);
    let version = peer.edit("big.bin", &Version::default(), Some(meta(5)));
    let added = peer.since(0, usize::MAX).entries;
    merge_peer(&mut project.task.shared.lock().saved.map, &added);
    let hash = Hash::from_bytes([5; 32]);

    // No machine can send it, so the next round has to pick it up, and the
    // collector may run before that.
    project.task.apply().await;
    assert_eq!(project.task.pending, 1);
    assert!(temp_roots(&project).await.contains(&hash));

    // The peer deleted it: what came is no use any more.
    peer.edit("big.bin", &version, None);
    let deleted = peer.since(0, usize::MAX).entries;
    merge_peer(&mut project.task.shared.lock().saved.map, &deleted);
    project.task.apply().await;
    assert!(!temp_roots(&project).await.contains(&hash));
    project.stop().await;
}

#[tokio::test]
async fn a_fetch_that_gets_no_data_gives_up() {
    let project = Project::new(&["notes.txt"]).await;
    let silent = Endpoint::builder(presets::Minimal).bind().await.unwrap();
    let router = iroh::protocol::Router::builder(silent)
        .accept(iroh_blobs::ALPN, Silent)
        .spawn();
    let connection = project
        .task
        .parts
        .endpoint
        .connect(router.endpoint().addr(), iroh_blobs::ALPN)
        .await
        .unwrap();
    let content = HashAndFormat::raw(Hash::new("never sent"));
    let stall = Duration::from_millis(200);
    let stop = CancellationToken::new();
    // The guard only keeps a regression from hanging the test run.
    let fetched = timeout(
        Duration::from_secs(20),
        fetch_within(&project.task.parts.store, connection, content, stall, &stop),
    )
    .await
    .expect("the fetch gives up on its own");
    let error = fetched.expect_err("nothing was sent");
    assert!(error.to_string().contains("no data came"), "{error:#}");
    router.shutdown().await.unwrap();
    project.stop().await;
}

#[tokio::test]
async fn keeping_the_original_deletes_the_copy() {
    let mut project = Project::new(&["docs/notes.txt", COPY]).await;
    project
        .task
        .shared
        .resolve(COPY, Keep::Original)
        .await
        .unwrap();
    assert!(!project.path(COPY).exists());
    assert_eq!(
        fs::read_to_string(project.path("docs/notes.txt")).unwrap(),
        "docs/notes.txt"
    );
    project.task.scan().await;
    assert_eq!(project.held(), [COPY]);
    project.stop().await;
}

#[tokio::test]
async fn only_an_unchanged_conflict_copy_is_settled() {
    let project = Project::new(&["a.txt", COPY]).await;
    let shared = &project.task.shared;
    let error = shared.resolve("a.txt", Keep::Copy).await.unwrap_err();
    assert!(error.to_string().contains("not a conflict copy"), "{error}");
    let error = shared.resolve("../x (conflict 1a2b3c4d)", Keep::Copy).await;
    assert!(error.is_err());
    // Edited since the scan: left alone.
    fs::write(project.path(COPY), "edited meanwhile").unwrap();
    let error = shared.resolve(COPY, Keep::Original).await.unwrap_err();
    assert!(error.to_string().contains("changed"), "{error}");
    assert!(project.path(COPY).exists());
    project.stop().await;
}

#[tokio::test]
async fn a_scan_that_finds_a_new_folder_asks_for_another() {
    fn asked(project: &Project) -> bool {
        let touched = project.task.shared.touched.notified();
        touched.now_or_never().is_some()
    }
    let mut project = Project::new(&["a.txt"]).await;
    assert!(asked(&project));
    project.task.scan().await;
    assert!(!asked(&project));
    fs::create_dir(project.path("new")).unwrap();
    project.task.scan().await;
    assert!(asked(&project));
    project.task.scan().await;
    assert!(!asked(&project));
    project.stop().await;
}

#[tokio::test]
async fn a_project_is_not_in_sync_before_its_peers_are_heard() {
    let project = Project::new(&["a.txt"]).await;
    let shared = &project.task.shared;
    let peer = SecretKey::from_bytes(&[9; 32]).public();
    shared.set_peers(vec![EndpointAddr::new(peer)]);
    shared.listen_again();
    project.task.publish_status(None);
    assert_eq!(shared.status().state, SyncState::Syncing);
    shared.heard(&peer);
    project.task.publish_status(None);
    assert_eq!(shared.status().state, SyncState::Idle);
    project.stop().await;
}

#[tokio::test]
async fn a_paused_project_says_so_and_settles_nothing() {
    let project = Project::new(&["a.txt", COPY]).await;
    project.task.shared.set_paused(true);
    project.task.publish_status(None);
    let status = project.task.shared.status();
    assert_eq!(status.state, SyncState::Paused);
    assert!(status.paused);
    assert_eq!(status.conflicts, [COPY]);
    let error = project
        .task
        .shared
        .resolve(COPY, Keep::Copy)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("paused"), "{error}");
    assert!(project.path(COPY).exists());
    project.task.shared.set_paused(false);
    project.task.publish_status(None);
    let status = project.task.shared.status();
    assert_eq!(status.state, SyncState::Idle);
    assert!(!status.paused);
    assert!(status.progress.is_none());
    project.stop().await;
}

#[tokio::test]
async fn files_deleted_then_the_folder_removed_send_no_deletes() {
    let mut project = Project::new(&["a.txt", "src/b.rs"]).await;
    // `rm -rf` midway: the files are gone, the folder and marker not yet.
    fs::remove_file(project.path("a.txt")).unwrap();
    fs::remove_dir_all(project.path("src")).unwrap();
    project.task.scan().await;
    assert_eq!(project.deleted(), Vec::<String>::new(), "held, not sent");
    assert_eq!(project.held(), ["a.txt", "src/b.rs"]);

    // Then the folder goes, within the settle window.
    fs::remove_dir_all(&project.folder).unwrap();
    project.task.send_deletes().await;
    assert_eq!(project.deleted(), Vec::<String>::new());
    let error = project.task.error.clone().unwrap_or_default();
    assert!(error.contains("not syncing"), "{error}");
    project.task.scan().await;
    project.task.send_deletes().await;
    assert_eq!(project.deleted(), Vec::<String>::new());
    project.stop().await;
}

#[tokio::test]
async fn the_marker_removed_with_the_files_sends_no_deletes() {
    let mut project = Project::new(&["a.txt", "b.txt"]).await;
    let marker = project.path(MARKER);
    fs::remove_file(project.path("a.txt")).unwrap();
    project.task.scan().await;
    assert_eq!(project.held(), ["a.txt"]);
    fs::remove_file(&marker).unwrap();
    fs::remove_file(project.path("b.txt")).unwrap();

    // Held deletes found the marker gone.
    project.task.send_deletes().await;
    assert_eq!(project.deleted(), Vec::<String>::new());
    assert!(project.task.scan_failed);
    // A scan does not trust the folder or mark it again.
    project.task.scan().await;
    project.task.send_deletes().await;
    assert_eq!(project.deleted(), Vec::<String>::new());
    assert!(project.task.scan_failed);
    assert!(!marker.exists(), "the marker is not written into it");
    project.stop().await;
}

#[tokio::test]
async fn a_subfolder_deleted_in_a_live_project_sends_its_deletes() {
    let mut project = Project::new(&["a.txt", "sub/x.txt", "sub/deep/y.txt"]).await;
    // The marker alone removed comes back, with no file missing.
    fs::remove_file(project.path(MARKER)).unwrap();
    project.task.scan().await;
    assert_eq!(project.task.error, None);
    assert!(project.path(MARKER).exists());

    fs::remove_dir_all(project.path("sub")).unwrap();
    project.task.scan().await;
    assert_eq!(project.deleted(), Vec::<String>::new(), "held first");
    // Quiet for the window: scanned again, then sent.
    project.task.scan().await;
    project.task.send_deletes().await;
    assert_eq!(project.task.error, None);
    assert_eq!(project.deleted(), ["sub/deep/y.txt", "sub/x.txt"]);
    assert!(project.held().is_empty());
    project.stop().await;
}

#[tokio::test]
async fn a_folder_moved_to_the_trash_sends_no_deletes() {
    let mut project = Project::new(&["a.txt", "b.txt"]).await;
    fs::remove_file(project.path("a.txt")).unwrap();
    project.task.scan().await;
    assert_eq!(project.held(), ["a.txt"]);
    let trash = project.base.join("Trash");
    fs::create_dir(&trash).unwrap();
    fs::rename(&project.folder, trash.join("game")).unwrap();

    project.task.send_deletes().await;
    assert_eq!(project.deleted(), Vec::<String>::new());
    assert!(project.task.scan_failed);
    project.task.scan().await;
    project.task.send_deletes().await;
    assert_eq!(project.deleted(), Vec::<String>::new());
    project.stop().await;
}

#[test]
fn held_deletes_wait_until_the_folder_is_quiet() {
    let start = Instant::now();
    let mut timer = DeleteTimer::default();
    // A change with nothing held starts no wait.
    timer.changed(start);
    assert_eq!(timer.due, None);
    timer.scanned(true, start);
    assert_eq!(timer.due, Some(start + DELETE_SETTLE));
    // Another scan still holding them keeps the time; a change moves it.
    let later = start + Duration::from_secs(3);
    timer.scanned(true, later);
    assert_eq!(timer.due, Some(start + DELETE_SETTLE));
    timer.changed(later);
    assert_eq!(timer.due, Some(later + DELETE_SETTLE));
    // Nothing missing any more: nothing is due.
    timer.scanned(false, later);
    assert_eq!(timer.due, None);
}

fn meta(hash: u8) -> FileMeta {
    FileMeta {
        hash: [hash; 32],
        size: 1,
        mtime_ms: 1,
        executable: false,
    }
}

/// Entries a peer made, one per path.
fn from_peer(paths: &[&str], hash: u8) -> Vec<Entry> {
    let mut peer = Map::new(7);
    for path in paths {
        peer.edit(path, &Version::default(), Some(meta(hash)));
    }
    peer.since(0, usize::MAX).entries
}

#[test]
fn a_peer_cannot_name_paths_outside_the_folder_or_in_git() {
    let bad = [
        ".git/hooks/pre-commit",
        "src/.GIT/config",
        "../x",
        "a/../../x",
        "./a",
        "/etc/passwd",
        "",
        "a/",
        "a//b",
        "a\\b",
        "a\0b",
        ".beep-1.partial",
    ];
    let mut map = Map::new(1);
    let mut entries = from_peer(&bad, 1);
    entries.extend(from_peer(&["src/ok.rs"], 2));
    let merged = merge_peer(&mut map, &entries);
    assert_eq!(
        merged,
        Merged {
            changed: true,
            refused: bad.len()
        }
    );
    let paths: Vec<_> = map.entries().map(|entry| entry.path.as_str()).collect();
    assert_eq!(paths, ["src/ok.rs"]);

    // Sent again, they are refused again and leave nothing to do, so
    // nothing is retried.
    let again = merge_peer(&mut map, &entries);
    assert!(!again.changed);
    let dir = tempfile::TempDir::new().unwrap();
    let state = State::new(Saved {
        map,
        ..Saved::default()
    });
    let work = plan(&state, &Rules::load(dir.path()).unwrap());
    let paths: Vec<_> = work.iter().map(|work| work.path.as_str()).collect();
    assert_eq!(paths, ["src/ok.rs"]);
}

#[test]
fn only_refs_are_removed_from_git_and_git_comes_in_the_order_it_needs() {
    let mut map = Map::new(1);
    let dir = TempDir::new().unwrap();
    let rules = Rules::load(dir.path()).unwrap();
    let seen_here = [
        ".git/objects/ab/0123456789012345678901234567890123456f",
        ".git/config",
        ".git/HEAD",
        ".git/refs/heads/gone",
    ];
    let mut state = State::new(Saved::default());
    for path in seen_here {
        map.edit(path, &Version::default(), Some(meta(1)));
        state.set_seen(
            path,
            Seen {
                stat: Stat {
                    size: 1,
                    mtime_ns: 0,
                    ctime_ns: 0,
                    inode: 1,
                    device: 1,
                    executable: false,
                },
                hash: [1; 32],
                version: map.get(path).unwrap().version.clone(),
            },
        );
    }
    // A peer deleted all of it, as `git gc` or `rm -rf .git` would.
    for path in seen_here {
        let base = map.get(path).unwrap().version.clone();
        map.edit(path, &base, None);
    }
    state.saved.map = map;
    let planned = plan(&state, &rules);
    let paths: Vec<_> = planned.iter().map(|work| work.path.as_str()).collect();
    assert_eq!(paths, [".git/refs/heads/gone"]);

    let mut map = Map::new(1);
    for path in [
        ".git/refs/heads/a",
        ".git/HEAD",
        ".git/objects/pack/pack-0a.idx",
        ".git/objects/pack/pack-0a.pack",
        "src/lib.rs",
    ] {
        map.edit(path, &Version::default(), Some(meta(2)));
    }
    let state = State::new(Saved {
        map,
        ..Saved::default()
    });
    let planned = plan(&state, &rules);
    let paths: Vec<_> = planned.iter().map(|work| work.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            ".git/objects/pack/pack-0a.pack",
            ".git/objects/pack/pack-0a.idx",
            "src/lib.rs",
            ".git/HEAD",
            ".git/refs/heads/a",
        ]
    );
}

#[test]
fn a_peer_cannot_grow_the_list_past_the_file_limit() {
    let mut map = Map::new(1);
    for n in 0..MAX_FILES {
        map.edit(&format!("f/{n}"), &Version::default(), Some(meta(1)));
    }
    let entries = from_peer(&["new.txt"], 2);
    assert_eq!(merge_peer(&mut map, &entries).refused, 1);
    assert!(map.get("new.txt").is_none());
    // An edit of a file it has is still taken.
    let mut peer = Map::new(7);
    let base = map.get("f/0").unwrap().version.clone();
    peer.edit("f/0", &base, Some(meta(3)));
    let merged = merge_peer(&mut map, &peer.since(0, 10).entries);
    assert_eq!(merged.refused, 0);
    assert_eq!(map.get("f/0").unwrap().file, Some(meta(3)));
}

#[tokio::test]
async fn a_file_changed_under_the_store_is_not_copied_as_its_old_content() {
    let dir = tempfile::TempDir::new().unwrap();
    let store = iroh_blobs::store::fs::FsStore::load(dir.path().join("store"))
        .await
        .unwrap();
    let path = dir.path().join("a.txt");
    // Big enough that the store reads it in place, not a copy.
    fs::write(&path, vec![b'a'; 100_000]).unwrap();
    let found = Found {
        path: "a.txt".into(),
        abs: path.clone(),
        stat: Stat::of(&fs::metadata(&path).unwrap()),
    };
    let tag = scan::import(&store, &found).await.unwrap();
    let hash = tag.hash();
    let stop = CancellationToken::new();
    let copy = |name: &str| {
        let file = fs::File::create(dir.path().join(name)).unwrap();
        AsyncFile::from_std(file)
    };

    let mut out = copy("one");
    let copied = copy_store(&store, hash, &mut out, &stop).await.unwrap();
    assert_eq!(copied, *hash.as_bytes());

    // Edited in place: the same file and size, other bytes.
    let mut file = fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.write_all(b"b").unwrap();
    drop(file);
    let mut out = copy("two");
    let copied = copy_store(&store, hash, &mut out, &stop).await.unwrap();
    assert_ne!(copied, *hash.as_bytes(), "the change is noticed");
    drop(tag);
    store.shutdown().await.ok();
}

#[test]
fn an_emptied_folder_found_at_start_deletes_nothing() {
    let gone = |missing: &[bool]| -> Vec<(String, bool)> {
        missing
            .iter()
            .enumerate()
            .map(|(n, missing)| (format!("f/{n}"), *missing))
            .collect()
    };
    // Every file missing at the first scan: no baseline.
    assert!(!has_baseline(true, 2, &gone(&[true, true])));
    // Some still there: the rest were deleted while the node was down.
    assert!(has_baseline(true, 3, &gone(&[true, true])));
    // Left out by the rules, not missing.
    assert!(has_baseline(true, 2, &gone(&[true, false])));
    // Emptied while it runs: the deletes are real.
    assert!(has_baseline(false, 2, &gone(&[true, true])));
    assert!(has_baseline(true, 0, &[]));
}

#[tokio::test]
async fn a_store_copy_that_cannot_be_read_is_fetched_again() {
    let dir = tempfile::TempDir::new().unwrap();
    let store = iroh_blobs::store::fs::FsStore::load(dir.path().join("store"))
        .await
        .unwrap();
    let path = dir.path().join("a.txt");
    let content = vec![b'a'; 100_000];
    // Big enough that the store reads it in place, not a copy.
    fs::write(&path, &content).unwrap();
    let found = Found {
        path: "a.txt".into(),
        abs: path.clone(),
        stat: Stat::of(&fs::metadata(&path).unwrap()),
    };
    let tag = scan::import(&store, &found).await.unwrap();
    let hash = tag.hash();
    let stop = CancellationToken::new();
    // The file the store reads from is gone.
    fs::remove_file(&path).unwrap();
    let mut out = AsyncFile::from_std(fs::File::create(dir.path().join("out")).unwrap());
    assert!(copy_store(&store, hash, &mut out, &stop).await.is_err());

    let mut refetched = false;
    let mut out = AsyncFile::from_std(fs::File::create(dir.path().join("out")).unwrap());
    copy_out(
        &store,
        hash,
        &mut out,
        &stop,
        async |out: &mut AsyncFile| {
            refetched = true;
            out.set_len(0).await?;
            out.seek(SeekFrom::Start(0)).await?;
            out.write_all(&content).await?;
            Ok(())
        },
    )
    .await
    .unwrap();
    out.flush().await.unwrap();
    assert!(refetched, "a peer is asked when the store fails");
    assert_eq!(fs::read(dir.path().join("out")).unwrap(), content);
    drop(tag);
    store.shutdown().await.ok();
}

#[test]
fn an_edit_after_a_won_conflict_builds_on_the_merge() {
    let stat = |inode| Stat {
        size: 1,
        mtime_ns: 0,
        ctime_ns: 0,
        inode,
        device: 1,
        executable: false,
    };
    let mut map = Map::new(1);
    let mine = map.edit("plan.md", &Version::default(), Some(meta(1)));
    // Another machine's concurrent edit loses to this one: the list
    // keeps this content under a version that has seen both.
    let mut other = Map::new(2);
    let older = FileMeta {
        mtime_ms: -1,
        ..meta(2)
    };
    let version = other.edit("plan.md", &Version::default(), Some(older.clone()));
    map.merge(&Entry {
        path: "plan.md".into(),
        version,
        file: Some(older),
    });
    let merged = map.get("plan.md").unwrap().version.clone();
    assert_eq!(merged.compare(&mine), Order::Newer);
    let mut state = State::new(Saved {
        map,
        ..Saved::default()
    });
    let seen = Seen {
        stat: stat(1),
        hash: [1; 32],
        version: mine,
    };
    state.set_seen("plan.md", seen);
    let found = Found {
        path: "plan.md".into(),
        abs: PathBuf::from("/srv/project/plan.md"),
        stat: stat(2),
    };
    let prior = state.saved.seen.get("plan.md").cloned();
    record(&mut state, &found, [3; 32], prior);
    let entry = state.saved.map.get("plan.md").unwrap();
    assert_eq!(entry.file.as_ref().map(|f| f.hash), Some([3; 32]));
    assert_eq!(entry.version.compare(&merged), Order::Newer);
    assert!(
        !state
            .saved
            .map
            .entries()
            .any(|e| is_conflict(&e.path) && e.file.as_ref().is_some_and(|f| f.hash == [3; 32])),
        "the edit is not a conflict"
    );
}

#[test]
fn content_never_moves_into_a_path_the_rules_skip() {
    let dir = tempfile::TempDir::new().unwrap();
    fs::write(dir.path().join("Cargo.toml"), "").unwrap();
    let rules = Rules::load(dir.path()).unwrap();
    let mut map = Map::new(1);
    map.edit("a.bin", &Version::default(), Some(meta(2)));
    map.edit("target/a.bin", &Version::default(), Some(meta(1)));
    let stat = Stat {
        size: 1,
        mtime_ns: 0,
        ctime_ns: 0,
        inode: 1,
        device: 1,
        executable: false,
    };
    let mut state = State::new(Saved {
        map,
        ..Saved::default()
    });
    state.set_seen(
        "a.bin",
        Seen {
            stat,
            hash: [1; 32],
            version: Version::default(),
        },
    );
    assert_eq!(move_target(&state, &rules, "a.bin"), None);
    state
        .saved
        .map
        .edit("kept.bin", &Version::default(), Some(meta(1)));
    assert_eq!(
        move_target(&state, &rules, "a.bin").as_deref(),
        Some("kept.bin")
    );
}
