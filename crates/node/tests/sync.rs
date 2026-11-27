//! Real nodes in this process sync one project folder node to node:
//! files go both ways, build output stays, deletes and conflicts carry
//! over, and a node that restarts mid-sync finishes without conflicts.
//!
//! The nodes react to their file watchers and to each other on their own
//! tasks, with nothing to wait on from outside, so the tests poll the
//! folders with a time limit.

use beep_node::files::{Folders, syncthing::Syncthing, transfer::Transfers};
use beep_proto::node::sync::{Keep, Project, ProjectStatus, Refused, Rule, State};
use beep_proto::watched;
use iroh::{Endpoint, EndpointAddr, SecretKey, endpoint::presets};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, UNIX_EPOCH},
};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::watch,
    task::JoinSet,
    time::{sleep, timeout},
};
use tokio_util::task::AbortOnDropHandle;

const PROJECT: &str = "game";
const LIMIT: Duration = Duration::from_secs(60);
const MARKER: &str = ".beep-sync";

#[path = "sync/background_git.rs"]
mod background_git;

/// One node: its key, its state and the folder it shares, which outlive
/// a restart.
struct Node {
    key: SecretKey,
    state: TempDir,
    shared: TempDir,
    running: Option<Running>,
}

struct Running {
    endpoint: Endpoint,
    transfers: Transfers,
}

impl Node {
    async fn new() -> Self {
        let shared = TempDir::new().expect("a temp folder");
        fs::create_dir(shared.path().join(PROJECT)).unwrap();
        let mut node = Self {
            key: SecretKey::generate(),
            state: TempDir::new().expect("a temp folder"),
            shared,
            running: None,
        };
        node.start().await;
        node
    }

    async fn start(&mut self) {
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(self.key.clone())
            .bind()
            .await
            .expect("a test endpoint binds");
        let folders = Folders::new(&[self.shared.path().to_owned()]);
        // Without a config in `syncthing_home`, only a marker in the shared
        // folder says Syncthing keeps the project in step.
        let syncthing = Syncthing::at(
            vec![self.syncthing_home()],
            Some(self.shared.path().to_owned()),
        );
        let transfers = Transfers::start_with(&endpoint, self.state.path(), folders, syncthing)
            .await
            .expect("the transfers start");
        self.running = Some(Running {
            endpoint,
            transfers,
        });
    }

    async fn stop(&mut self) {
        if let Some(running) = self.running.take() {
            running.transfers.shutdown().await.expect("the node stops");
            running.endpoint.close().await;
        }
    }

    /// Where the node looks for Syncthing's `config.xml`.
    fn syncthing_home(&self) -> PathBuf {
        self.state.path().join("syncthing")
    }

    fn running(&self) -> &Running {
        self.running.as_ref().expect("the node runs")
    }

    fn addr(&self) -> EndpointAddr {
        self.running().endpoint.addr()
    }

    fn folder(&self) -> PathBuf {
        self.shared
            .path()
            .join(PROJECT)
            .canonicalize()
            .expect("the project folder exists")
    }

    fn path(&self, path: &str) -> PathBuf {
        self.folder().join(path)
    }

    /// Syncs the project with `peer`.
    async fn sync_with(&self, peer: &Node) {
        let refused = self.configure(vec![peer.addr()]).await;
        assert!(refused.is_empty(), "{refused:?}");
    }

    /// Sets the project as the server would, with `peers`.
    async fn configure(&self, peers: Vec<EndpointAddr>) -> Vec<Refused> {
        self.configure_paused(peers, false).await
    }

    async fn configure_paused(&self, peers: Vec<EndpointAddr>, paused: bool) -> Vec<Refused> {
        self.running()
            .transfers
            .syncs()
            .configure(vec![Project {
                name: PROJECT.into(),
                folder: self.folder().to_string_lossy().into_owned(),
                peers,
                paused,
                rules: String::new(),
            }])
            .await
    }

    async fn pause(&self, peer: &Node, paused: bool) {
        let refused = self.configure_paused(vec![peer.addr()], paused).await;
        assert!(refused.is_empty(), "{refused:?}");
    }

    /// Marks the shared folder, above the project, as Syncthing's.
    fn mark_syncthing(&self, on: bool) {
        let marker = self.shared.path().join(".stfolder");
        if on {
            fs::create_dir_all(marker).unwrap();
        } else {
            fs::remove_dir_all(marker).unwrap();
        }
    }

    fn status(&self) -> ProjectStatus {
        let mut status = self.running().transfers.syncs().status();
        assert_eq!(status.len(), 1, "{status:?}");
        status.remove(0)
    }
}

async fn pair() -> (Node, Node) {
    let a = Node::new().await;
    let b = Node::new().await;
    a.sync_with(&b).await;
    b.sync_with(&a).await;
    (a, b)
}

/// Waits until `check` holds, looking again every few milliseconds.
async fn eventually(what: &str, mut check: impl FnMut() -> bool) {
    let waited = timeout(LIMIT, async {
        while !check() {
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await;
    assert!(waited.is_ok(), "timed out waiting for {what}");
}

fn read(path: &Path) -> Option<Vec<u8>> {
    fs::read(path).ok()
}

fn write(path: &Path, data: impl AsRef<[u8]>) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, data).unwrap();
}

fn set_mtime(path: &Path, secs: u64) {
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_secs(secs))
        .unwrap();
}

/// Every file under `dir`, by relative path, sorted.
fn files(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_owned()];

    while let Some(at) = stack.pop() {
        for entry in fs::read_dir(&at).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                stack.push(entry.path());
            } else if entry.file_name() == MARKER {
                // What tells the folder from another, never synced.
                continue;
            } else {
                let path = entry.path();
                out.push(
                    path.strip_prefix(dir)
                        .unwrap()
                        .to_string_lossy()
                        // As sync names them, on Windows too.
                        .replace('\\', "/"),
                );
            }
        }
    }

    out.sort();
    out
}

/// Data no two files in a test share.
fn data(len: usize, seed: u8) -> Vec<u8> {
    (0..len).map(|n| (n % 251) as u8 ^ seed).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_node_does_not_count_a_peer_it_cannot_reach_as_caught_up() {
    let (mut a, b) = pair().await;
    let a_id = a.addr().id;
    a.stop().await;
    let waited = timeout(
        Duration::from_secs(3),
        b.running().transfers.syncs().have_all_from(
            PROJECT,
            &a_id,
            tokio::time::Instant::now(),
            |_| {},
        ),
    )
    .await;
    assert!(waited.is_err(), "it ended without hearing from the peer");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_syncs_both_ways_and_build_output_stays() {
    let (a, b) = pair().await;
    a.pause(&b, true).await;
    b.pause(&a, true).await;
    eventually("both nodes to pause before preparing metadata", || {
        a.status().paused && b.status().paused
    })
    .await;
    write(&a.path("Cargo.toml"), "[package]\nname = \"game\"\n");
    write(&a.path("src/main.rs"), "fn main() {}\n");
    write(&a.path("assets/big.bin"), data(300_000, 1));
    write(&a.path("target/debug/game"), data(1000, 2));
    #[cfg(unix)]
    fs::set_permissions(
        a.path("src/main.rs"),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    set_mtime(&a.path("assets/big.bin"), 1_700_000_000);

    a.pause(&b, false).await;
    b.pause(&a, false).await;
    eventually("the files to reach the other node", || {
        read(&b.path("assets/big.bin")) == Some(data(300_000, 1))
            && read(&b.path("src/main.rs")).is_some()
            && read(&b.path("Cargo.toml")).is_some()
    })
    .await;
    let landed = fs::metadata(b.path("assets/big.bin")).unwrap();
    assert_eq!(
        landed.modified().unwrap(),
        UNIX_EPOCH + Duration::from_secs(1_700_000_000)
    );
    #[cfg(unix)]
    {
        let mode = std::os::unix::fs::PermissionsExt::mode(
            &fs::metadata(b.path("src/main.rs")).unwrap().permissions(),
        );
        assert_ne!(mode & 0o100, 0, "the executable bit carries over");
    }

    // The other way.
    write(&b.path("src/main.rs"), "fn main() { println!(\"hi\"); }\n");
    eventually("the edit to come back", || {
        read(&a.path("src/main.rs")).as_deref()
            == Some(b"fn main() { println!(\"hi\"); }\n".as_slice())
    })
    .await;
    // Build output never left the first node.
    assert!(!b.path("target").exists());
    assert_eq!(
        files(&b.folder()),
        ["Cargo.toml", "assets/big.bin", "src/main.rs"]
    );

    let skipped = a
        .running()
        .transfers
        .syncs()
        .skipped(PROJECT, 100)
        .await
        .unwrap();
    let target = skipped
        .paths
        .iter()
        .find(|path| path.path == "target")
        .expect("target is listed as left out");
    assert!(target.folder);
    assert_eq!(target.rule, Rule::Default);
    assert_eq!(target.marker.as_deref(), Some("Cargo.toml"));
    let status = a.running().transfers.syncs().status();
    assert_eq!(status.len(), 1);
    assert_eq!(status[0].files, 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_delete_carries_over_and_clears_its_folder() {
    let (a, b) = pair().await;
    write(&a.path("notes/today.txt"), "remember");
    write(&a.path("keep.txt"), "kept");
    eventually("the files to arrive", || {
        read(&b.path("notes/today.txt")).is_some() && read(&b.path("keep.txt")).is_some()
    })
    .await;
    fs::remove_file(a.path("notes/today.txt")).unwrap();
    eventually("the delete to arrive", || {
        !b.path("notes/today.txt").exists()
    })
    .await;
    file_round(&a, &b).await;
    assert!(!b.path("notes").exists(), "the empty folder goes too");
    assert_eq!(
        read(&b.path("keep.txt")).as_deref(),
        Some(b"kept".as_slice())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn edits_made_on_both_machines_keep_both() {
    let (a, mut b) = pair().await;
    write(&a.path("plan.md"), "first");
    eventually("the file to arrive", || {
        read(&b.path("plan.md")).as_deref() == Some(b"first".as_slice())
    })
    .await;
    // Wait until the second node recorded it, so its edit is an edit of
    // that version and not a new file.
    eventually("the second node to settle", || {
        b.running().transfers.syncs().status()[0].files == 1
            && b.running().transfers.syncs().status()[0].pending == 0
    })
    .await;

    // Both edit it while apart.
    b.stop().await;
    write(&a.path("plan.md"), "from a");
    set_mtime(&a.path("plan.md"), 2_000_000_000);
    write(&b.path("plan.md"), "from b");
    set_mtime(&b.path("plan.md"), 1_900_000_000);
    b.start().await;
    a.sync_with(&b).await;
    b.sync_with(&a).await;

    let both = |node: &Node| {
        let names = files(&node.folder());
        names.len() == 2
            && read(&node.path("plan.md")).as_deref() == Some(b"from a".as_slice())
            && names
                .iter()
                .filter(|name| name.starts_with("plan (conflict "))
                .any(|name| read(&node.path(name)).as_deref() == Some(b"from b".as_slice()))
    };
    eventually("both edits on both machines", || both(&a) && both(&b)).await;
    assert_eq!(files(&a.folder()), files(&b.folder()));
    let conflicts = &a.running().transfers.syncs().status()[0].conflicts;
    assert_eq!(conflicts.len(), 1, "{conflicts:?}");
    assert!(conflicts[0].ends_with(".md"));

    // Keeping the copy on one machine settles it on both.
    a.running()
        .transfers
        .syncs()
        .resolve(PROJECT, &conflicts[0], Keep::Copy)
        .await
        .expect("the conflict settles");
    let settled = |node: &Node| {
        files(&node.folder()) == ["plan.md"]
            && read(&node.path("plan.md")).as_deref() == Some(b"from b".as_slice())
    };
    eventually("the kept copy on both machines", || {
        settled(&a) && settled(&b)
    })
    .await;
    eventually("no conflict left", || {
        a.status().conflicts.is_empty() && b.status().conflicts.is_empty()
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_paused_project_catches_up_when_resumed() {
    let (a, b) = pair().await;
    synced_file(&a, &b).await;
    a.pause(&b, true).await;
    b.pause(&a, true).await;
    eventually("both to say they are paused", || {
        let (sa, sb) = (a.status(), b.status());
        sa.paused && sb.paused && sa.state == State::Paused && sb.state == State::Paused
    })
    .await;
    write(&a.path("later.txt"), "while paused");
    let files_before = a.status().files;
    // Settling a conflict waits for the resume.
    let refused = a
        .running()
        .transfers
        .syncs()
        .resolve(PROJECT, "x (conflict 01020304).txt", Keep::Original)
        .await
        .expect_err("refused while paused");
    assert!(refused.to_string().contains("paused"), "{refused}");
    assert_eq!(
        a.status().files,
        files_before,
        "nothing scanned while paused"
    );

    a.pause(&b, false).await;
    b.pause(&a, false).await;
    eventually("the file written while paused to arrive", || {
        read(&b.path("later.txt")).as_deref() == Some(b"while paused".as_slice())
    })
    .await;
    assert!(!b.status().paused);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_swapped_for_an_empty_one_deletes_nothing() {
    let (mut a, b) = pair().await;
    write(&a.path("notes.txt"), "keep me");
    eventually("the file to arrive", || {
        read(&b.path("notes.txt")).as_deref() == Some(b"keep me".as_slice())
    })
    .await;
    eventually("the first node to settle", || {
        let status = &a.running().transfers.syncs().status()[0];
        status.files == 1 && status.pending == 0
    })
    .await;
    assert!(a.folder().join(MARKER).exists());

    // The folder is swapped for an empty one while the node is down, as
    // an unmounted disk leaves its mount point.
    a.stop().await;
    let folder = a.folder();
    fs::rename(&folder, folder.with_file_name("moved away")).unwrap();
    fs::create_dir(&folder).unwrap();
    a.start().await;

    eventually("the first node to refuse the folder", || {
        let status = a.running().transfers.syncs().status();
        status.first().is_some_and(|status| {
            status.state == State::Failed
                && status
                    .error
                    .as_deref()
                    .is_some_and(|error| error.contains("not mounted"))
        })
    })
    .await;
    // Nothing was deleted anywhere, and nothing written into the new one.
    let status = &b.running().transfers.syncs().status()[0];
    assert_eq!(status.files, 1);
    assert_eq!(
        read(&b.path("notes.txt")).as_deref(),
        Some(b"keep me".as_slice())
    );
    assert!(files(&a.folder()).is_empty());
    assert!(!a.folder().join(MARKER).exists());

    // Started over, the new folder is filled and nothing is deleted.
    a.running()
        .transfers
        .syncs()
        .start_over(PROJECT)
        .await
        .expect("the project starts over");
    nothing_deleted(&a, &b).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_replaced_while_syncing_starts_over_and_deletes_nothing() {
    let (a, b) = pair().await;
    synced_file(&a, &b).await;

    // The folder is moved aside and a new one made at its path while the
    // node runs, as a fresh clone does: sync refuses it.
    let folder = a.folder();
    fs::rename(&folder, folder.with_file_name("moved away")).unwrap();
    fs::create_dir(&folder).unwrap();
    let refused = a
        .running()
        .transfers
        .syncs()
        .rescan(PROJECT)
        .await
        .expect("the folder scans");
    assert_eq!(refused.state, State::Failed, "{refused:?}");
    assert!(
        refused
            .error
            .as_deref()
            .is_some_and(|error| error.contains("is another folder now")),
        "{refused:?}"
    );

    a.running()
        .transfers
        .syncs()
        .start_over(PROJECT)
        .await
        .expect("the project starts over");

    nothing_deleted(&a, &b).await;
    assert!(a.folder().join(MARKER).exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_node_taken_out_of_a_project_stops_getting_its_changes() {
    let (a, b) = pair().await;
    let syncs = a.running().transfers.syncs();
    eventually("the second node to pull from the first", || {
        syncs.pulls_served() == 1
    })
    .await;
    // The first node syncs the project with nobody now.
    let refused = syncs
        .configure(vec![Project {
            name: PROJECT.into(),
            folder: a.folder().to_string_lossy().into_owned(),
            peers: Vec::new(),
            paused: false,
            rules: String::new(),
        }])
        .await;
    assert!(refused.is_empty(), "{refused:?}");
    eventually("the pull to end", || syncs.pulls_served() == 0).await;
    drop(b);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_named_another_way_keeps_syncing() {
    let (a, b) = pair().await;
    synced_file(&a, &b).await;
    // The same folder, written with a trailing slash: the project starts
    // again on it and must still see its changes.
    let refused = a
        .running()
        .transfers
        .syncs()
        .configure(vec![Project {
            name: PROJECT.into(),
            folder: format!("{}/", a.folder().display()),
            peers: vec![b.addr()],
            paused: false,
            rules: String::new(),
        }])
        .await;
    assert!(refused.is_empty(), "{refused:?}");
    write(&a.path("later.txt"), "after the change");
    eventually("the change made after it to arrive", || {
        read(&b.path("later.txt")).as_deref() == Some(b"after the change".as_slice())
    })
    .await;
}

/// Writes a file on `a`, waits for it on `b`, and for `a` to settle.
async fn synced_file(a: &Node, b: &Node) {
    write(&a.path("notes.txt"), "keep me");
    eventually("the file to arrive", || {
        read(&b.path("notes.txt")).as_deref() == Some(b"keep me".as_slice())
    })
    .await;
    eventually("the first node to settle", || {
        let status = &a.running().transfers.syncs().status()[0];
        status.files == 1 && status.pending == 0
    })
    .await;
}

/// Empties `node`'s folder, leaving the marker that tells it apart.
fn empty_folder(node: &Node) {
    for entry in fs::read_dir(node.folder()).unwrap() {
        let entry = entry.unwrap();

        if entry.file_name() == MARKER {
            continue;
        }

        if entry.file_type().unwrap().is_dir() {
            fs::remove_dir_all(entry.path()).unwrap();
        } else {
            fs::remove_file(entry.path()).unwrap();
        }
    }
}

/// `b` keeps the file and `a` gets it back.
async fn nothing_deleted(a: &Node, b: &Node) {
    eventually("the file to come back", || {
        read(&a.path("notes.txt")).as_deref() == Some(b"keep me".as_slice())
    })
    .await;
    assert_eq!(
        read(&b.path("notes.txt")).as_deref(),
        Some(b"keep me".as_slice())
    );
    let status = &b.running().transfers.syncs().status()[0];
    assert_eq!(status.files, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_project_turned_off_and_emptied_deletes_nothing_when_turned_on() {
    let (a, b) = pair().await;
    synced_file(&a, &b).await;

    let refused = a.running().transfers.syncs().configure(Vec::new()).await;
    assert!(refused.is_empty(), "{refused:?}");
    empty_folder(&a);
    a.sync_with(&b).await;

    nothing_deleted(&a, &b).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_emptied_while_the_node_is_down_deletes_nothing() {
    let (mut a, b) = pair().await;
    synced_file(&a, &b).await;

    a.stop().await;
    empty_folder(&a);
    a.start().await;

    nothing_deleted(&a, &b).await;
}

/// The project's folder on `node`, whether it is there or not.
fn folder_path(node: &Node) -> PathBuf {
    node.shared.path().canonicalize().unwrap().join(PROJECT)
}

/// `b` still has the file after scanning its folder again.
async fn kept_on(b: &Node) {
    let status = b
        .running()
        .transfers
        .syncs()
        .rescan(PROJECT)
        .await
        .expect("the folder scans");
    assert_eq!(status.files, 1, "{status:?}");
    assert_eq!(
        read(&b.path("notes.txt")).as_deref(),
        Some(b"keep me".as_slice())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_deleted_while_syncing_is_made_again_and_filled() {
    let (a, b) = pair().await;
    synced_file(&a, &b).await;

    fs::remove_dir_all(folder_path(&a)).unwrap();
    kept_on(&b).await;
    a.running().transfers.syncs().check_folders().await;

    nothing_deleted(&a, &b).await;
    kept_on(&b).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_deleted_while_the_node_is_down_is_made_again_and_filled() {
    let (mut a, b) = pair().await;
    synced_file(&a, &b).await;

    a.stop().await;
    fs::remove_dir_all(folder_path(&a)).unwrap();
    kept_on(&b).await;
    a.start().await;

    nothing_deleted(&a, &b).await;
    kept_on(&b).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_project_taken_off_a_machine_and_its_folder_deleted_there_deletes_nothing_elsewhere() {
    let (a, b) = pair().await;
    synced_file(&a, &b).await;

    let refused = a.running().transfers.syncs().configure(Vec::new()).await;
    assert!(refused.is_empty(), "{refused:?}");
    fs::remove_dir_all(folder_path(&a)).unwrap();
    a.running().transfers.syncs().check_folders().await;

    assert!(
        !folder_path(&a).exists(),
        "a project taken off is not made again"
    );
    kept_on(&b).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_syncthing_keeps_is_refused() {
    let a = Node::new().await;
    a.mark_syncthing(true);
    let refused = a.configure(Vec::new()).await;
    assert_eq!(refused.len(), 1, "{refused:?}");
    let shared = a.shared.path().canonicalize().unwrap();
    let label = shared.file_name().unwrap().to_string_lossy();
    assert_eq!(
        refused[0].message,
        format!("Already synced by Syncthing ({label})")
    );
    assert!(a.running().transfers.syncs().status().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_project_the_server_sets_outside_the_shared_folders_is_refused() {
    let a = Node::new().await;
    let outside = TempDir::new().unwrap();
    let refused = a
        .running()
        .transfers
        .syncs()
        .configure(vec![Project {
            name: PROJECT.into(),
            folder: outside.path().to_string_lossy().into_owned(),
            peers: Vec::new(),
            paused: false,
            rules: String::new(),
        }])
        .await;
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(
        refused[0].message.contains("cannot be synced"),
        "{refused:?}"
    );
    assert!(a.running().transfers.syncs().status().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_synced_folder_syncthing_takes_over_stops_and_deletes_nothing() {
    let (a, b) = pair().await;
    synced_file(&a, &b).await;

    a.mark_syncthing(true);
    a.running().transfers.syncs().check_syncthing().await;
    let held = a.status();
    assert_eq!(held.state, State::Failed);
    assert!(
        held.error
            .as_deref()
            .unwrap_or_default()
            .starts_with("Already synced by Syncthing"),
        "{held:?}"
    );
    // The server sets it again, as it does on every connect: still held.
    assert!(a.configure(vec![b.addr()]).await.is_empty());
    assert_eq!(a.status().state, State::Failed);

    // What changes while Syncthing has it is not Beep's to carry.
    empty_folder(&a);
    a.mark_syncthing(false);
    a.running().transfers.syncs().check_syncthing().await;
    nothing_deleted(&a, &b).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_node_restarting_on_a_folder_syncthing_took_holds_it() {
    let (mut a, b) = pair().await;
    synced_file(&a, &b).await;

    a.stop().await;
    a.mark_syncthing(true);
    a.start().await;
    let held = a.status();
    assert_eq!(held.state, State::Failed, "{held:?}");
    assert!(held.error.is_some());
    assert_eq!(
        read(&b.path("notes.txt")).as_deref(),
        Some(b"keep me".as_slice())
    );
}

/// What a fake Syncthing says of its one folder: paused or not, and the
/// number of its last event.
#[derive(Clone, Copy)]
struct FakeFolder {
    paused: bool,
    event: u64,
}

fn set_paused(folder: &watch::Sender<FakeFolder>, paused: bool) {
    folder.send_modify(|folder| {
        folder.paused = paused;
        folder.event += 1;
    });
}

/// A fake Syncthing GUI listing one folder at `path`, with `/rest/events`
/// held open until the folder changes, and the `config.xml` in `home` that
/// points a node at it.
async fn fake_syncthing(
    home: &Path,
    path: &Path,
    folder: watch::Sender<FakeFolder>,
) -> AbortOnDropHandle<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    fs::create_dir_all(home).unwrap();
    let config = format!(
        r#"<configuration><gui tls="false"><address>{addr}</address><apikey>fake-key</apikey></gui></configuration>"#
    );
    fs::write(home.join("config.xml"), config).unwrap();
    let path = serde_json::to_string(&path.to_string_lossy()).unwrap();
    let task = tokio::spawn(async move {
        let mut answering = JoinSet::new();

        while let Ok((stream, _)) = listener.accept().await {
            answering.spawn(fake_answer(stream, folder.subscribe(), path.clone()));
        }
    });
    AbortOnDropHandle::new(task)
}

async fn fake_answer(mut stream: TcpStream, mut folder: watch::Receiver<FakeFolder>, path: String) {
    let mut request = Vec::new();
    let mut buf = [0; 1024];

    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => request.extend_from_slice(&buf[..n]),
        }
    }

    let text = String::from_utf8_lossy(&request);
    let line = text.lines().next().unwrap_or_default();
    let body = if let Some((_, query)) = line.split_once("since=") {
        let since: u64 = query.split('&').next().unwrap().parse().unwrap();
        watched::wait_for(&mut folder, |folder| folder.event > since)
            .await
            .unwrap();
        let event = watched::read(&folder, |folder| folder.event);
        format!(r#"[{{"id":{event},"type":"FolderResumed"}}]"#)
    } else {
        let paused = watched::read(&folder, |folder| folder.paused);
        format!(r#"[{{"id":"game-1","label":"Game","path":{path},"paused":{paused}}}]"#)
    };
    let reply = format!(
        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(reply.as_bytes()).await.unwrap();
}

/// Waits for the error on the project to be `want`.
/// The limit is well under the check's one minute: only the event can do it.
async fn settles_on(node: &Node, want: Option<&str>) {
    let settled = timeout(
        Duration::from_secs(10),
        eventually("the check to follow Syncthing", || {
            let status = node.running().transfers.syncs().status();
            status.len() == 1 && status[0].error.as_deref() == want
        }),
    )
    .await;
    assert!(settled.is_ok(), "the check did not follow the event");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_paused_in_syncthing_syncs_through_beep_until_resumed() {
    let mut a = Node::new().await;
    let b = Node::new().await;
    let (folder, _) = watch::channel(FakeFolder {
        paused: true,
        event: 0,
    });
    let _api = fake_syncthing(&a.syncthing_home(), &a.folder(), folder.clone()).await;
    // Started again to find the config, as a node started after Syncthing.
    a.stop().await;
    a.start().await;
    // Syncthing's marker, which must not reach the peer.
    write(&a.path(".stfolder/syncthing-folder-1.txt"), "game-1");
    a.sync_with(&b).await;
    b.sync_with(&a).await;
    synced_file(&a, &b).await;
    assert!(!b.path(".stfolder").exists());

    // Resumed: Beep's sync stops as soon as Syncthing says so.
    set_paused(&folder, false);
    settles_on(&a, Some("Already synced by Syncthing (Game)")).await;
    write(&a.path("meanwhile.txt"), "Syncthing's to carry");

    // Paused again: Beep takes it back, and what changed meanwhile arrives.
    set_paused(&folder, true);
    settles_on(&a, None).await;
    write(&a.path("later.txt"), "while paused");
    eventually("the files written since to arrive", || {
        read(&b.path("later.txt")).as_deref() == Some(b"while paused".as_slice())
            && read(&b.path("meanwhile.txt")).is_some()
    })
    .await;
}

/// Runs git in `dir` and gives what it printed. It takes no optional locks,
/// so a test looking on never makes sync's own git wait.
fn git(dir: &Path, args: &[&str]) -> String {
    let output = try_git(dir, args);
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn try_git(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .current_dir(dir)
        // Each test's repository, never one found above it.
        .env("GIT_CEILING_DIRECTORIES", dir.parent().unwrap())
        .args([
            "--no-optional-locks",
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
        ])
        .args(args)
        .output()
        .expect("git runs")
}

/// A repository with one commit in the node's project folder.
fn new_repo(node: &Node) {
    git(&node.folder(), &["init", "-q", "-b", "main"]);
    commit(node, "README.md", "first\n");
}

/// Commits `text` to `file` and says which commit that made.
fn commit(node: &Node, file: &str, text: &str) -> String {
    let dir = node.folder();
    write(&dir.join(file), text);
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", file]);
    git(&dir, &["rev-parse", "HEAD"])
}

fn head(node: &Node) -> Option<String> {
    let output = try_git(
        &node.folder(),
        &["rev-parse", "-q", "--verify", "HEAD^{commit}"],
    );
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn has_ref(node: &Node, name: &str) -> bool {
    try_git(&node.folder(), &["rev-parse", "-q", "--verify", name])
        .status
        .success()
}

/// Whether the checkout is one git calls clean.
fn is_clean(node: &Node) -> bool {
    git(&node.folder(), &["status", "--porcelain"]).is_empty()
}

fn assert_sound(node: &Node) {
    let dir = node.folder();
    // Refs are read by `for-each-ref` below: fsck's own ref check takes the
    // `.lock` file of a ref the node is updating for a ref with bad content.
    let fsck = try_git(&dir, &["fsck", "--full", "--strict", "--no-references"]);
    assert!(
        fsck.status.success(),
        "{}",
        String::from_utf8_lossy(&fsck.stderr)
    );
    git(&dir, &["for-each-ref"]);
    git(&dir, &["log", "--all", "--oneline"]);
    let names = files(&dir.join(".git"));
    assert!(
        !names.iter().any(|name| name.contains(" (conflict ")),
        "{names:?}"
    );
}

/// Complete a fresh peer round before asserting that edits stayed local.
async fn file_round(source: &Node, target: &Node) {
    source
        .running()
        .transfers
        .syncs()
        .rescan(PROJECT)
        .await
        .unwrap();
    timeout(
        LIMIT,
        target.running().transfers.syncs().have_all_from(
            PROJECT,
            &source.addr().id,
            tokio::time::Instant::now(),
            |_| {},
        ),
    )
    .await
    .unwrap()
    .unwrap();
}

async fn in_step(a: &Node, b: &Node) {
    eventually("the checkouts to reach one commit and be clean", || {
        head(a).is_some() && head(a) == head(b) && is_clean(a) && is_clean(b)
    })
    .await;
    assert_sound(a);
    assert_sound(b);
}

/// A repository on `a`, on `b` too once they sync.
async fn synced_repo() -> (Node, Node) {
    let a = Node::new().await;
    let b = Node::new().await;
    new_repo(&a);
    a.sync_with(&b).await;
    b.sync_with(&a).await;
    in_step(&a, &b).await;
    (a, b)
}

#[tokio::test(flavor = "multi_thread")]
async fn git_copies_once_then_peer_commits_sync_and_staging_stays_local() {
    let a = Node::new().await;
    let b = Node::new().await;
    new_repo(&a);
    write(&a.shared.path().join(".stignore"), ".git/\n");
    write(&b.shared.path().join(".stignore"), ".git/\n");
    write(&a.path(".gitignore"), "local.env\ntarget/built.o\n");
    write(&a.path("local.env"), "ignored, and stays local\n");
    // Tracked, in a folder a default leaves out: `git status` would say
    // deleted without it.
    write(&a.path("target/kept.txt"), "tracked\n");
    write(&a.path("target/built.o"), "not tracked\n");
    git(&a.folder(), &["add", ".gitignore", "target/kept.txt"]);
    git(&a.folder(), &["commit", "-q", "-m", "more"]);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("README.md", a.path("link")).unwrap();
        git(&a.folder(), &["add", "link"]);
        git(&a.folder(), &["commit", "-q", "-m", "a link"]);
    }
    a.sync_with(&b).await;
    b.sync_with(&a).await;
    in_step(&a, &b).await;
    assert!(!b.path("local.env").exists());
    assert!(read(&b.path("target/built.o")).is_none());
    assert!(!b.path(".git/index.lock").exists());

    let there = commit(&a, "src/lib.rs", "pub fn a() {}\n");
    eventually("peer commits to arrive without an upstream", || {
        head(&b).as_deref() == Some(there.as_str()) && is_clean(&b)
    })
    .await;
    assert_eq!(
        read(&b.path("src/lib.rs")).as_deref(),
        Some(b"pub fn a() {}\n".as_slice())
    );

    // Working edits, branches, tags, config and the index stay local.
    git(&b.folder(), &["tag", "v1"]);
    git(&b.folder(), &["branch", "side"]);
    git(&b.folder(), &["pack-refs", "--all"]);
    write(&b.path("README.md"), "staged here\n");
    git(&b.folder(), &["add", "README.md"]);
    write(&b.path("local.env"), "after Git changes\n");
    file_round(&b, &a).await;
    assert_eq!(
        read(&a.path("local.env")).as_deref(),
        Some(b"ignored, and stays local\n".as_slice())
    );
    assert_ne!(read(&a.path("README.md")), read(&b.path("README.md")));
    assert!(!has_ref(&a, "refs/tags/v1"));
    assert!(!has_ref(&a, "refs/heads/side"));
    assert!(git(&a.folder(), &["diff", "--cached", "--name-only"]).is_empty());
    assert_eq!(
        git(&b.folder(), &["diff", "--cached", "--name-only"]),
        "README.md"
    );
    assert_sound(&a);
    assert_sound(&b);
}

/// A folder that already has the project's files and no `.git`, as another
/// file sync leaves it: the history arrives, and what is there stays.

#[tokio::test(flavor = "multi_thread")]
async fn divergent_commits_wait_until_git_merges_them() {
    let (a, b) = synced_repo().await;
    a.pause(&b, true).await;
    b.pause(&a, true).await;
    let ours = commit(&a, "ours.txt", "ours\n");
    let theirs = commit(&b, "theirs.txt", "theirs\n");
    a.pause(&b, false).await;
    b.pause(&a, false).await;
    eventually("both checkouts to wait for Git", || {
        [&a, &b].iter().all(|node| {
            node.status()
                .error
                .as_deref()
                .is_some_and(|error| error.contains("histories diverged"))
        })
    })
    .await;
    assert_eq!(head(&a).as_deref(), Some(ours.as_str()));
    assert_eq!(head(&b).as_deref(), Some(theirs.as_str()));
    assert!(!a.path("theirs.txt").exists());
    assert!(!b.path("ours.txt").exists());
    assert!(git(&a.folder(), &["for-each-ref", "refs/beep-conflicts"]).is_empty());
    assert!(git(&b.folder(), &["for-each-ref", "refs/beep-conflicts"]).is_empty());
    git(
        &a.folder(),
        &["fetch", &b.folder().to_string_lossy(), "main"],
    );
    git(&a.folder(), &["merge", "--no-edit", "FETCH_HEAD"]);
    git(
        &b.folder(),
        &["pull", "--ff-only", &a.folder().to_string_lossy(), "main"],
    );
    in_step(&a, &b).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unresolved_merge_warns_and_a_resolved_merge_resumes_sync() {
    let (a, b) = synced_repo().await;
    a.pause(&b, true).await;
    b.pause(&a, true).await;
    let ours = commit(&a, "README.md", "local intent\n");
    let theirs = commit(&b, "README.md", "peer intent\n");
    a.pause(&b, false).await;
    b.pause(&a, false).await;
    eventually("divergent histories to warn", || {
        a.status()
            .error
            .as_deref()
            .is_some_and(|e| e.contains("histories diverged"))
    })
    .await;
    git(
        &a.folder(),
        &["fetch", &b.folder().to_string_lossy(), "main"],
    );
    let merge = Command::new("git")
        .current_dir(a.folder())
        .args(["merge", "--no-edit", "FETCH_HEAD"])
        .output()
        .unwrap();
    assert!(!merge.status.success(), "the fixture must conflict");
    eventually("unresolved merge to warn rather than wait silently", || {
        a.status()
            .error
            .as_deref()
            .is_some_and(|e| e.contains("unmerged conflicts"))
    })
    .await;
    assert_eq!(head(&a).as_deref(), Some(ours.as_str()));
    assert_eq!(head(&b).as_deref(), Some(theirs.as_str()));
    assert!(
        fs::read_to_string(a.path("README.md"))
            .unwrap()
            .contains("<<<<<<<")
    );
    fs::write(a.path("README.md"), "local intent and peer intent\n").unwrap();
    git(&a.folder(), &["add", "README.md"]);
    git(&a.folder(), &["commit", "-q", "-m", "resolve both intents"]);
    in_step(&a, &b).await;
    assert_eq!(head(&a), head(&b));
    assert_eq!(
        fs::read_to_string(b.path("README.md")).unwrap(),
        "local intent and peer intent\n"
    );
    eventually("resolved merge warnings to clear", || {
        a.status().error.is_none() && b.status().error.is_none()
    })
    .await;
}

async fn configure_three(nodes: &[Node; 3], paused: bool) {
    for (i, node) in nodes.iter().enumerate() {
        let peers = nodes
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, peer)| peer.addr())
            .collect();
        assert!(node.configure_paused(peers, paused).await.is_empty());
    }

    eventually("all three nodes to acknowledge pause or resume", || {
        nodes.iter().all(|node| node.status().paused == paused)
    })
    .await;
}

fn conflict_refs(node: &Node) -> String {
    git(
        &node.folder(),
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/beep-conflicts",
        ],
    )
}

fn working_files(node: &Node) -> Option<Vec<(String, Vec<u8>)>> {
    let root = node.folder();
    let mut stack = vec![root.clone()];
    let mut out = Vec::new();

    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(dir).ok()? {
            let entry = entry.ok()?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // The observer must not walk git's locks or sync's temporary
            // folders, which can disappear while a transfer finishes.
            if name == ".git" || name.starts_with(".beep-") {
                continue;
            }

            if entry.file_type().ok()?.is_dir() {
                stack.push(entry.path());
            } else {
                let path = entry.path();
                out.push((
                    path.strip_prefix(&root)
                        .ok()?
                        .to_string_lossy()
                        .replace('\\', "/"),
                    fs::read(&path).ok()?,
                ));
            }
        }
    }

    out.sort();
    Some(out)
}

/// Commits start at a barrier while sync is paused, so all three branch
/// from the same tip before any peer sees another writer's changes.
fn concurrent_commits(nodes: &[Node; 3], round: usize, shared: bool, tied: bool) -> Vec<String> {
    let barrier = std::sync::Barrier::new(3);
    std::thread::scope(|scope| {
        let jobs: Vec<_> = nodes
            .iter()
            .enumerate()
            .map(|(i, node)| {
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    let own = format!("host-{i}/round-{round}.txt");
                    let common = format!("shared-{round}.txt");
                    let text = format!("round {round}, host {i}\n");
                    // Adjacent and equal timestamps must both require Git
                    // reconciliation rather than choosing the latest writer.
                    let seconds =
                        2_000_000_000 + round as u64 * 10 + if tied { 0 } else { i as u64 };
                    write(&node.path(&own), &text);
                    set_mtime(&node.path(&own), seconds);
                    let mut paths = vec![own.as_str()];

                    if shared {
                        write(&node.path(&common), &text);
                        set_mtime(&node.path(&common), seconds);
                        paths.push(common.as_str());
                    }

                    let mut add = vec!["add", "--"];
                    add.extend(paths);
                    git(&node.folder(), &add);
                    let date = format!("{seconds} +0000");
                    let output = Command::new("git")
                        .current_dir(node.folder())
                        .env("GIT_AUTHOR_DATE", &date)
                        .env("GIT_COMMITTER_DATE", &date)
                        .args([
                            "-c",
                            "user.name=test",
                            "-c",
                            "user.email=test@example.com",
                            "-c",
                            "commit.gpgsign=false",
                            "commit",
                            "-q",
                            "-m",
                            &text,
                        ])
                        .output()
                        .unwrap();
                    assert!(
                        output.status.success(),
                        "{}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                    head(node).unwrap()
                })
            })
            .collect();
        jobs.into_iter().map(|job| job.join().unwrap()).collect()
    })
}

async fn three_host_commits_wait_for_git(shared: bool, tied: bool) {
    let mut nodes = [Node::new().await, Node::new().await, Node::new().await];
    new_repo(&nodes[0]);

    for round in 0..3 {
        write(&nodes[0].path(&format!("shared-{round}.txt")), "baseline\n");
    }

    git(
        &nodes[0].folder(),
        &["add", "--", "shared-0.txt", "shared-1.txt", "shared-2.txt"],
    );
    git(
        &nodes[0].folder(),
        &["commit", "-q", "-m", "shared baseline"],
    );
    configure_three(&nodes, false).await;
    eventually("three clean initial Git copies", || {
        let tip = head(&nodes[0]);
        tip.is_some()
            && nodes
                .iter()
                .all(|node| head(node) == tip && is_clean(node) && node.status().error.is_none())
    })
    .await;

    for round in 0..3 {
        configure_three(&nodes, true).await;
        let tips = concurrent_commits(&nodes, round, shared, tied);
        let trees: Vec<_> = nodes.iter().map(working_files).collect();
        let indexes: Vec<_> = nodes
            .iter()
            .map(|node| read(&node.path(".git/index")))
            .collect();
        assert!(trees.iter().all(Option::is_some));
        assert!(indexes.iter().all(Option::is_some));
        configure_three(&nodes, false).await;
        eventually("all three divergent checkouts to wait for Git", || {
            nodes.iter().all(|node| {
                node.status()
                    .error
                    .as_deref()
                    .is_some_and(|error| error.contains("Git commits differ"))
            })
        })
        .await;

        if round == 1 {
            nodes[2].stop().await;
            nodes[2].start().await;
            configure_three(&nodes, false).await;
            eventually("the restarted node to keep waiting for Git", || {
                nodes[2]
                    .status()
                    .error
                    .as_deref()
                    .is_some_and(|error| error.contains("Git commits differ"))
            })
            .await;
        }

        for (i, node) in nodes.iter().enumerate() {
            assert_eq!(head(node).as_ref(), tips.get(i));
            assert_eq!(working_files(node), trees[i]);
            assert_eq!(read(&node.path(".git/index")), indexes[i]);
            assert!(conflict_refs(node).is_empty());
            assert!(node.status().conflicts.is_empty());
            assert_sound(node);
        }

        println!(
            "three hosts, shared={shared}, tied={tied}, round={round}: commits differ, file sync waits; each HEAD, index and working tree unchanged"
        );

        // Reconcile through Git, resolving shared-file conflicts explicitly.
        // File sync must neither choose a branch tip nor resolve the merge.
        configure_three(&nodes, true).await;

        for peer in &nodes[1..] {
            git(
                &nodes[0].folder(),
                &["fetch", &peer.folder().to_string_lossy(), "main"],
            );
            let merge = try_git(&nodes[0].folder(), &["merge", "--no-edit", "FETCH_HEAD"]);
            if shared {
                assert_eq!(
                    merge.status.code(),
                    Some(1),
                    "{}",
                    String::from_utf8_lossy(&merge.stderr)
                );
                let common = format!("shared-{round}.txt");
                assert_eq!(
                    git(
                        &nodes[0].folder(),
                        &["diff", "--name-only", "--diff-filter=U"]
                    ),
                    common
                );
                let text = format!("resolved round {round}\n");
                write(&nodes[0].path(&common), text);
                git(&nodes[0].folder(), &["add", "--", &common]);
                git(
                    &nodes[0].folder(),
                    &["commit", "-q", "-m", "resolve shared file"],
                );
            } else {
                assert!(
                    merge.status.success(),
                    "{}",
                    String::from_utf8_lossy(&merge.stderr)
                );
            }
        }

        for peer in &nodes[1..] {
            git(
                &peer.folder(),
                &[
                    "pull",
                    "--ff-only",
                    &nodes[0].folder().to_string_lossy(),
                    "main",
                ],
            );
        }

        configure_three(&nodes, false).await;
        eventually(
            "file sync to resume after Git agrees on all three hosts",
            || {
                let tip = head(&nodes[0]);
                tip.is_some()
                    && nodes.iter().all(|node| {
                        head(node) == tip
                            && is_clean(node)
                            && node.status().pending == 0
                            && node.status().error.is_none()
                    })
            },
        )
        .await;

        for node in &nodes {
            let history = git(&node.folder(), &["log", "--all", "--format=%H"]);
            assert!(
                tips.iter()
                    .all(|tip| history.lines().any(|line| line == tip))
            );

            for i in 0..3 {
                assert_eq!(
                    read(&node.path(&format!("host-{i}/round-{round}.txt"))),
                    Some(format!("round {round}, host {i}\n").into_bytes())
                );
            }

            assert!(conflict_refs(node).is_empty());
            assert!(node.status().conflicts.is_empty());
            assert_sound(node);
        }

        println!(
            "three hosts, round={round}: Git merged all three commits; all checkouts clean, no sync conflict copies or refs"
        );
    }

    let heads: Vec<_> = nodes.iter().map(head).collect();
    write(&nodes[0].path("resumed.txt"), "file sync resumed\n");

    for node in &nodes[1..] {
        file_round(&nodes[0], node).await;
        assert!(!node.path("resumed.txt").exists());
    }

    for (i, node) in nodes.iter_mut().enumerate() {
        assert_eq!(head(node), heads[i]);
        assert_sound(node);
        node.stop().await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn three_hosts_with_separate_file_commits_wait_for_git() {
    three_host_commits_wait_for_git(false, false).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn three_hosts_with_shared_file_commits_wait_for_git() {
    three_host_commits_wait_for_git(true, false).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn three_hosts_with_equal_commit_times_wait_for_git() {
    three_host_commits_wait_for_git(true, true).await;
}

/// Two checkouts of one commit, as Syncthing leaves them when it pauses.
async fn twins() -> (Node, Node) {
    let a = Node::new().await;
    let b = Node::new().await;
    new_repo(&a);
    git(&b.folder(), &["init", "-q", "-b", "main"]);
    git(
        &b.folder(),
        &["pull", "-q", &a.folder().to_string_lossy(), "main"],
    );
    (a, b)
}

#[tokio::test(flavor = "multi_thread")]
async fn existing_checkouts_advance_inactive_branches_and_keep_their_indexes() {
    let (a, b) = twins().await;
    let original = head(&b).unwrap();
    git(&a.folder(), &["branch", "side"]);
    git(&b.folder(), &["branch", "side"]);
    // The branch checked out on `a` syncs to `b`, which has it too.
    git(&a.folder(), &["switch", "-q", "side"]);
    let ahead = commit(&a, "side.txt", "side\n");
    write(&b.path("README.md"), "staged locally\n");
    git(&b.folder(), &["add", "README.md"]);
    a.sync_with(&b).await;
    b.sync_with(&a).await;
    file_round(&b, &a).await;
    eventually("the inactive side branch to fast-forward", || {
        git(&b.folder(), &["rev-parse", "refs/heads/side"]) == ahead
    })
    .await;
    assert_ne!(read(&a.path("README.md")), read(&b.path("README.md")));
    assert_eq!(git(&a.folder(), &["rev-parse", "refs/heads/side"]), ahead);
    assert_eq!(git(&b.folder(), &["rev-parse", "refs/heads/side"]), ahead);
    assert_eq!(head(&b).as_deref(), Some(original.as_str()));
    assert_eq!(
        read(&b.path("README.md")).as_deref(),
        Some(b"staged locally\n".as_slice())
    );
    assert_eq!(
        git(&b.folder(), &["diff", "--cached", "--name-only"]),
        "README.md"
    );
    assert!(git(&a.folder(), &["diff", "--cached", "--name-only"]).is_empty());
    assert_sound(&a);
    assert_sound(&b);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_copy_interrupted_part_way_finishes_and_never_looks_corrupt() {
    let a = Node::new().await;
    let mut b = Node::new().await;
    new_repo(&a);

    for n in 0..30 {
        commit(
            &a,
            &format!("files/{n}.txt"),
            &format!("{n}\n").repeat(5000),
        );
    }

    write(&a.path("big.bin"), data(4_000_000, 7));
    git(&a.folder(), &["add", "big.bin"]);
    git(&a.folder(), &["commit", "-q", "-m", "big"]);
    a.sync_with(&b).await;
    b.sync_with(&a).await;

    // Git sees no repository yet, or a sound one, at any point: a ref only
    // ever names what is here.
    let sound_so_far = |b: &Node| {
        if b.path(".git/HEAD").exists() && b.path(".git/refs").exists() {
            let fsck = try_git(&b.folder(), &["fsck", "--connectivity-only"]);
            assert!(
                fsck.status.success(),
                "{}",
                String::from_utf8_lossy(&fsck.stderr)
            );
        }
    };
    eventually("part of the copy to arrive", || {
        b.path(".git/objects").exists() || b.path("files").exists()
    })
    .await;
    b.stop().await;
    sound_so_far(&b);
    b.start().await;
    b.sync_with(&a).await;
    a.sync_with(&b).await;
    eventually("the copy to finish", || {
        sound_so_far(&b);
        head(&a) == head(&b) && is_clean(&b)
    })
    .await;
    in_step(&a, &b).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_linked_worktree_is_copied_as_a_branch_only_during_bootstrap() {
    let a = Node::new().await;
    let b = Node::new().await;
    new_repo(&a);
    write(&a.path(".gitignore"), ".worktrees/\nwt/\n");
    git(&a.folder(), &["add", ".gitignore"]);
    git(&a.folder(), &["commit", "-q", "-m", "ignore"]);
    git(
        &a.folder(),
        &["worktree", "add", "-q", ".worktrees/feat", "-b", "feat"],
    );
    git(
        &a.folder(),
        &["worktree", "add", "-q", "wt/other", "-b", "other"],
    );
    let feat = a.path(".worktrees/feat");
    write(&feat.join("work.txt"), "on feat\n");
    git(&feat, &["add", "-A"]);
    git(&feat, &["commit", "-q", "-m", "feat"]);
    let tip = git(&feat, &["rev-parse", "HEAD"]);
    a.sync_with(&b).await;
    b.sync_with(&a).await;
    in_step(&a, &b).await;
    eventually("the branch of the worktree to arrive", || {
        has_ref(&b, "refs/heads/feat")
    })
    .await;
    assert_eq!(git(&b.folder(), &["rev-parse", "refs/heads/feat"]), tip);
    assert!(!b.path(".worktrees").exists());
    assert!(!b.path("wt").exists());
    assert!(!b.path(".git/worktrees").exists());
    assert!(git(&feat, &["status", "--porcelain"]).is_empty());

    // A later commit on that branch belongs to Git and moves no local ref.
    git(&b.folder(), &["switch", "-q", "feat"]);
    let moved = commit(&b, "elsewhere.txt", "elsewhere\n");
    eventually("the status to say what waits", || {
        a.status()
            .error
            .as_deref()
            .is_some_and(|error| error.contains("Git commits differ"))
    })
    .await;
    assert_eq!(git(&feat, &["rev-parse", "HEAD"]), tip);
    assert_ne!(moved, tip);
    assert!(git(&feat, &["status", "--porcelain"]).is_empty());
    assert_sound(&a);
}

#[tokio::test(flavor = "multi_thread")]
async fn gc_and_new_objects_stay_local_after_the_first_copy() {
    let (a, b) = synced_repo().await;
    let before = files(&b.path(".git/objects"));
    git(&a.folder(), &["gc", "-q", "--prune=now"]);
    write(&a.path("round.txt"), "after gc\n");
    file_round(&a, &b).await;
    assert!(!b.path("round.txt").exists());
    assert_eq!(files(&b.path(".git/objects")), before);
    fs::remove_file(a.path("round.txt")).unwrap();

    assert_sound(&a);
    assert_sound(&b);
    let next = commit(&a, "after.txt", "after gc\n");
    eventually("the new commit to arrive from the peer", || {
        head(&b).as_deref() == Some(next.as_str()) && is_clean(&b)
    })
    .await;
    assert_eq!(
        read(&b.path("after.txt")).as_deref(),
        Some(b"after gc\n".as_slice())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_node_on_the_old_protocol_is_turned_away() {
    let (a, b) = synced_repo().await;
    // Branches only: sync may still record the peer's tip under refs/remotes.
    let refs = git(&a.folder(), &["show-ref", "--heads"]);
    for alpn in [
        &b"beep-sync/6"[..],
        &b"beep-sync/1"[..],
        b"beep-sync/2",
        b"beep-sync/3",
        b"beep-sync/4",
        b"beep-sync/5",
    ] {
        let old = b.running().endpoint.connect(a.addr(), alpn).await;
        assert!(old.is_err(), "an old node is not served");
    }
    assert_sound(&a);
    assert_eq!(git(&a.folder(), &["show-ref", "--heads"]), refs);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_peer_that_never_reported_git_deletes_nothing_of_it() {
    // A peer leaves `.git` out, as a node from before this did, and reports
    // the rest of the folder.
    let a = Node::new().await;
    let b = Node::new().await;
    new_repo(&a);
    let refs = git(&a.folder(), &["show-ref"]);
    write(&b.path(".beepignore"), ".git/\n");
    write(&b.path("theirs.txt"), "from a peer without git\n");
    a.sync_with(&b).await;
    b.sync_with(&a).await;
    file_round(&b, &a).await;
    file_round(&a, &b).await;
    assert!(!a.path("theirs.txt").exists());
    assert!(!b.path("README.md").exists());
    assert_sound(&a);
    assert_eq!(git(&a.folder(), &["show-ref"]), refs);
    assert!(!b.path(".git").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn two_existing_checkouts_at_other_commits_keep_their_files() {
    let a = Node::new().await;
    let b = Node::new().await;
    new_repo(&a);
    new_repo(&b);
    let ours = commit(&a, "a.txt", "only on a\n");
    let theirs = commit(&b, "b.txt", "only on b\n");
    a.sync_with(&b).await;
    b.sync_with(&a).await;
    eventually("both to report different Git commits", || {
        [&a, &b].iter().all(|node| {
            node.status()
                .error
                .as_deref()
                .is_some_and(|error| error.contains("Git commits differ"))
        })
    })
    .await;
    assert_eq!(head(&a).as_deref(), Some(ours.as_str()));
    assert_eq!(head(&b).as_deref(), Some(theirs.as_str()));
    assert!(!a.path("b.txt").exists());
    assert!(!b.path("a.txt").exists());
    assert_sound(&a);
    assert_sound(&b);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_nested_project_taken_over_from_a_paused_share_honors_its_stignore() {
    let mut a = Node::new().await;
    let b = Node::new().await;
    // The Syncthing folder is the whole share, above the project.
    let share = a.shared.path().canonicalize().unwrap();
    let (folder, _) = watch::channel(FakeFolder {
        paused: true,
        event: 0,
    });
    let _api = fake_syncthing(&a.syncthing_home(), &share, folder.clone()).await;
    a.stop().await;
    a.start().await;
    write(&share.join(".stfolder/syncthing-folder-1.txt"), "game-1");
    write(
        &share.join(".stignore"),
        "// Local data.\n(?d)/game/data/captures\n(?i)*.RAW\n!keep.log\n*.log\n",
    );
    write(&a.path("data/captures/take.mov"), data(1000, 1));
    write(&a.path("data/notes.txt"), "goes along\n");
    write(&a.path("shot.raw"), "stays\n");
    write(&a.path("keep.log"), "goes along\n");
    write(&a.path("other.log"), "stays\n");
    a.sync_with(&b).await;
    b.sync_with(&a).await;
    eventually("what the rules keep to arrive", || {
        read(&b.path("data/notes.txt")).is_some() && read(&b.path("keep.log")).is_some()
    })
    .await;
    assert!(!b.path("data/captures").exists());
    assert!(!b.path("shot.raw").exists());
    assert!(!b.path("other.log").exists());
    assert!(a.path("data/captures/take.mov").exists());

    // Syncthing resumes the share: Beep's sync lets go at once.
    set_paused(&folder, false);
    settles_on(&a, Some("Already synced by Syncthing (Game)")).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stignore_it_cannot_follow_refuses_the_folder_and_says_why() {
    let a = Node::new().await;
    let b = Node::new().await;
    write(&a.shared.path().join(".stignore"), "(?x)odd\n");
    write(&a.path("secret.bin"), "must not travel\n");
    let refused = a.configure(vec![b.addr()]).await;
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(
        refused[0].message.contains("does not take over"),
        "{refused:?}"
    );
    assert!(refused[0].message.contains("(?x)odd"), "{refused:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_config_from_a_peer_brings_the_remote_and_nothing_git_runs() {
    let a = Node::new().await;
    let b = Node::new().await;
    new_repo(&a);
    let dir = a.folder();
    git(
        &dir,
        &["remote", "add", "origin", "https://example.com/app.git"],
    );
    for (key, value) in [
        ("core.fsmonitor", "/tmp/payload"),
        ("core.sshCommand", "/tmp/payload"),
        ("core.hooksPath", "/tmp/hooks"),
        ("alias.x", "!/tmp/payload"),
        ("credential.helper", "!/tmp/payload"),
        ("filter.x.clean", "/tmp/payload"),
        ("diff.x.textconv", "/tmp/payload"),
        ("remote.evil.url", "ext::sh -c /tmp/payload"),
    ] {
        git(&dir, &["config", key, value]);
    }
    a.sync_with(&b).await;
    b.sync_with(&a).await;
    eventually("the checkout and its config to arrive", || {
        head(&b).is_some() && read(&b.path(".git/config")).is_some()
    })
    .await;
    let config = String::from_utf8(read(&b.path(".git/config")).unwrap()).unwrap();
    assert!(config.contains("https://example.com/app.git"), "{config}");
    assert!(config.contains("repositoryformatversion"), "{config}");
    for hostile in [
        "fsmonitor",
        "sshcommand",
        "hookspath",
        "alias",
        "credential",
        "filter",
        "textconv",
        "payload",
        "ext::",
    ] {
        assert!(
            !config.to_lowercase().contains(hostile),
            "{hostile} in\n{config}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unborn_repository_is_copied_only_once() {
    let a = Node::new().await;
    let mut b = Node::new().await;
    git(&a.folder(), &["init", "-q", "-b", "main"]);
    a.sync_with(&b).await;
    b.sync_with(&a).await;
    eventually("an unborn checkout to arrive", || {
        try_git(&b.folder(), &["rev-parse", "--git-dir"])
            .status
            .success()
            && b.status().pending == 0
    })
    .await;
    b.stop().await;
    b.start().await;
    b.sync_with(&a).await;
    a.sync_with(&b).await;
    commit(&a, "first.txt", "first\n");
    eventually("the first commit to wait for a Git pull", || {
        b.status()
            .error
            .as_deref()
            .is_some_and(|error| error.contains("Git commits differ"))
    })
    .await;
    assert_eq!(head(&b), None);
    assert!(!b.path("first.txt").exists());
    git(
        &b.folder(),
        &["pull", "--ff-only", &a.folder().to_string_lossy(), "main"],
    );
    in_step(&a, &b).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_clean_checkout_fetches_commits_without_copying_uncommitted_edits() {
    let (a, b) = synced_repo().await;
    a.pause(&b, true).await;
    b.pause(&a, true).await;
    git(
        &b.folder(),
        &["remote", "add", "origin", &a.folder().to_string_lossy()],
    );
    git(&b.folder(), &["fetch", "origin"]);
    git(
        &b.folder(),
        &["branch", "--set-upstream-to=origin/main", "main"],
    );
    let target = commit(&a, "README.md", "committed change\n");
    write(&a.path("README.md"), "work after the commit\n");
    a.pause(&b, false).await;
    b.pause(&a, false).await;
    eventually("Git to catch up without carrying the working edit", || {
        head(&b).as_deref() == Some(target.as_str())
            && read(&b.path("README.md")).as_deref() == Some(b"committed change\n".as_slice())
            && b.status().pending == 0
    })
    .await;
    assert!(b.status().conflicts.is_empty(), "{:?}", b.status());
    assert!(git(&b.folder(), &["diff", "--cached", "--name-only"]).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn three_peers_catch_up_to_the_newest_linear_commit() {
    let nodes = [Node::new().await, Node::new().await, Node::new().await];
    new_repo(&nodes[0]);
    configure_three(&nodes, false).await;
    eventually("the initial checkout on every peer", || {
        let tip = head(&nodes[0]);
        tip.is_some() && nodes.iter().all(|node| head(node) == tip && is_clean(node))
    })
    .await;
    configure_three(&nodes, true).await;
    commit(&nodes[0], "one.txt", "one\n");
    git(
        &nodes[1].folder(),
        &["fetch", &nodes[0].folder().to_string_lossy(), "main"],
    );
    git(&nodes[1].folder(), &["merge", "--ff-only", "FETCH_HEAD"]);
    let newest = commit(&nodes[1], "two.txt", "two\n");
    configure_three(&nodes, false).await;
    eventually("all peers to reach the newest commit", || {
        nodes
            .iter()
            .all(|node| head(node).as_deref() == Some(newest.as_str()) && is_clean(node))
    })
    .await;

    for node in &nodes {
        assert_eq!(
            read(&node.path("one.txt")).as_deref(),
            Some(b"one\n".as_slice())
        );
        assert_eq!(
            read(&node.path("two.txt")).as_deref(),
            Some(b"two\n".as_slice())
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn peer_commits_do_not_move_a_different_branch() {
    let (a, b) = synced_repo().await;
    a.pause(&b, true).await;
    b.pause(&a, true).await;
    git(&b.folder(), &["switch", "-c", "other"]);
    let before = head(&b);
    let newest = commit(&a, "new.txt", "new commit\n");
    a.pause(&b, false).await;
    b.pause(&a, false).await;
    eventually(
        "inactive main to catch up without moving the checkout",
        || git(&b.folder(), &["rev-parse", "main"]) == newest,
    )
    .await;
    assert_eq!(head(&b), before);
    assert!(!b.path("new.txt").exists());
    assert_eq!(git(&b.folder(), &["branch", "--show-current"]), "other");
}

#[tokio::test(flavor = "multi_thread")]
async fn automatic_git_catchup_preserves_unstaged_staged_and_untracked_work() {
    for kind in ["unstaged", "staged", "untracked"] {
        let (a, b) = synced_repo().await;
        a.pause(&b, true).await;
        b.pause(&a, true).await;
        git(
            &b.folder(),
            &["remote", "add", "origin", &a.folder().to_string_lossy()],
        );
        git(&b.folder(), &["fetch", "origin"]);
        git(
            &b.folder(),
            &["branch", "--set-upstream-to=origin/main", "main"],
        );
        let before = head(&b);
        let path = if kind == "untracked" {
            "local.txt"
        } else {
            "README.md"
        };
        write(&b.path(path), "local work\n");

        if kind == "staged" {
            git(&b.folder(), &["add", path]);
        }

        let status = git(&b.folder(), &["status", "--porcelain"]);
        commit(&a, "README.md", "new commit\n");
        a.pause(&b, false).await;
        b.pause(&a, false).await;
        eventually("the dirty checkout to report why it waits", || {
            b.status()
                .error
                .as_deref()
                .is_some_and(|e| e.contains("commit or stash"))
        })
        .await;
        assert_eq!(head(&b), before);
        assert_eq!(
            read(&b.path(path)).as_deref(),
            Some(b"local work\n".as_slice())
        );
        assert_eq!(git(&b.folder(), &["status", "--porcelain"]), status);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn explicit_handoff_brings_edits_and_live_sync_does_not_undo_them() {
    let (a, b) = synced_repo().await;
    write(&a.path("README.md"), "work in progress\n");
    git(&a.folder(), &["add", "README.md"]);
    write(&a.path("new.bin"), [0, 1, 255]);
    file_round(&a, &b).await;
    assert_ne!(read(&a.path("README.md")), read(&b.path("README.md")));
    assert!(!b.path("new.bin").exists());
    let snapshot = a
        .running()
        .transfers
        .syncs()
        .handoff_snapshot(PROJECT)
        .await
        .unwrap();
    b.running()
        .transfers
        .syncs()
        .handoff_apply(PROJECT, snapshot)
        .await
        .unwrap();
    file_round(&a, &b).await;
    file_round(&b, &a).await;
    assert_eq!(read(&a.path("README.md")), read(&b.path("README.md")));
    assert_eq!(read(&b.path("new.bin")), Some(vec![0, 1, 255]));
    assert_eq!(
        git(&a.folder(), &["diff", "--cached", "--name-only"]),
        "README.md"
    );
    assert!(git(&b.folder(), &["diff", "--cached", "--name-only"]).is_empty());
    assert!(a.status().conflicts.is_empty());
    assert!(b.status().conflicts.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn git_recovery_keeps_staged_unstaged_untracked_and_ignored_work() {
    let (a, b) = synced_repo().await;
    commit(&a, "local.txt", "one\ntwo\nthree\n");
    commit(&a, ".gitignore", "*.cache\n");
    in_step(&a, &b).await;
    a.pause(&b, true).await;
    b.pause(&a, true).await;
    write(&b.path("local.txt"), "staged\ntwo\nthree\n");
    git(&b.folder(), &["add", "local.txt"]);
    write(&b.path("local.txt"), "staged\ntwo\nunstaged\n");
    write(&b.path("notes/local.bin"), [0, 2, 255]);
    write(&b.path("render.cache"), "ignored local data");
    let staged = git(&b.folder(), &["diff", "--cached", "--binary"]);
    let unstaged = git(&b.folder(), &["diff", "--binary"]);
    let target = commit(&a, "upstream.txt", "committed remotely\n");
    git(&b.folder(), &["config", "user.name", "Test"]);
    git(&b.folder(), &["config", "user.email", "test@example.com"]);
    b.running()
        .transfers
        .syncs()
        .set_git_auto_recovery([PROJECT.to_owned()].into());
    a.pause(&b, false).await;
    b.pause(&a, false).await;
    eventually("automatic recovery to restore all local work", || {
        head(&b).as_deref() == Some(target.as_str())
            && read(&b.path("notes/local.bin")) == Some(vec![0, 2, 255])
            && !b.path(".git/beep-recovery/pending.json").exists()
            && !git(&b.folder(), &["stash", "list"]).is_empty()
    })
    .await;
    assert_eq!(git(&b.folder(), &["diff", "--cached", "--binary"]), staged);
    assert_eq!(git(&b.folder(), &["diff", "--binary"]), unstaged);
    assert_eq!(
        read(&b.path("render.cache")).as_deref(),
        Some(b"ignored local data".as_slice())
    );
    assert!(!a.path("notes/local.bin").exists());
    let backup = git(&b.folder(), &["rev-parse", "refs/stash"]);
    let private_blobs = [
        git(
            &b.folder(),
            &["rev-parse", &format!("{backup}^2:local.txt")],
        ),
        git(
            &b.folder(),
            &["rev-parse", &format!("{backup}^3:notes/local.bin")],
        ),
    ];
    assert!(!git(&b.folder(), &["for-each-ref", "refs/beep-recovery"]).is_empty());
    // Packed stashes must remain local when a new machine imports the checkout.
    a.pause(&b, true).await;
    b.pause(&a, true).await;
    git(&b.folder(), &["gc", "--quiet"]);
    let c = Node::new().await;
    b.configure(vec![c.addr()]).await;
    c.configure(vec![b.addr()]).await;
    eventually("a new checkout to receive only committed content", || {
        head(&c).as_deref() == Some(target.as_str()) && is_clean(&c)
    })
    .await;
    assert!(!c.path("notes/local.bin").exists());
    assert!(git(&c.folder(), &["stash", "list"]).is_empty());
    assert!(git(&c.folder(), &["for-each-ref", "refs/beep-recovery"]).is_empty());
    assert!(
        !try_git(&c.folder(), &["cat-file", "-e", &backup])
            .status
            .success()
    );

    for blob in private_blobs {
        assert!(
            !try_git(&c.folder(), &["cat-file", "-e", &blob])
                .status
                .success()
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn git_recovery_conflicts_keep_the_backup_and_do_not_repeat_after_restart() {
    let (a, mut b) = synced_repo().await;
    a.pause(&b, true).await;
    b.pause(&a, true).await;
    write(&b.path("README.md"), "local work\n");
    write(&b.path("local.bin"), [0, 9, 255]);
    let target = commit(&a, "README.md", "remote work\n");
    git(&b.folder(), &["config", "user.name", "Test"]);
    git(&b.folder(), &["config", "user.email", "test@example.com"]);
    b.running()
        .transfers
        .syncs()
        .set_git_auto_recovery([PROJECT.to_owned()].into());
    a.pause(&b, false).await;
    b.pause(&a, false).await;
    eventually("a recovery conflict warning", || {
        b.status()
            .error
            .as_deref()
            .is_some_and(|error| error.contains("Git sync recovery needs review"))
    })
    .await;
    assert_eq!(head(&b).as_deref(), Some(target.as_str()));
    let backup = git(&b.folder(), &["rev-parse", "refs/stash"]);
    assert!(git(&b.folder(), &["show", &format!("{backup}:README.md")]).contains("local work"));
    assert_eq!(
        try_git(&b.folder(), &["show", &format!("{backup}^3:local.bin")]).stdout,
        [0, 9, 255]
    );
    b.stop().await;
    b.start().await;
    b.running()
        .transfers
        .syncs()
        .set_git_auto_recovery([PROJECT.to_owned()].into());
    eventually(
        "the pending recovery to remain visible after restart",
        || {
            b.status()
                .error
                .as_deref()
                .is_some_and(|error| error.contains("Git sync recovery needs review"))
        },
    )
    .await;
    assert_eq!(git(&b.folder(), &["rev-parse", "refs/stash"]), backup);
    assert!(b.path(".git/beep-recovery/pending.json").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn git_recovery_never_stashes_for_divergent_histories() {
    let (a, b) = synced_repo().await;
    a.pause(&b, true).await;
    b.pause(&a, true).await;
    commit(&a, "one.txt", "remote commit\n");
    let before = commit(&b, "two.txt", "local commit\n");
    write(&b.path("local.txt"), "uncommitted local work\n");
    git(&b.folder(), &["config", "user.name", "Test"]);
    git(&b.folder(), &["config", "user.email", "test@example.com"]);
    b.running()
        .transfers
        .syncs()
        .set_git_auto_recovery([PROJECT.to_owned()].into());
    a.pause(&b, false).await;
    b.pause(&a, false).await;
    eventually("divergent histories to require review", || {
        b.status()
            .error
            .as_deref()
            .is_some_and(|error| error.contains("diverged"))
    })
    .await;
    assert_eq!(head(&b).as_deref(), Some(before.as_str()));
    assert!(!b.path(".git/beep-recovery").exists());
    assert!(git(&b.folder(), &["stash", "list"]).is_empty());
    assert_eq!(
        read(&b.path("local.txt")).as_deref(),
        Some(b"uncommitted local work\n".as_slice())
    );
}

async fn ignored_config(node: &Node, peer: &Node, enabled: bool, paused: bool) {
    use beep_proto::node::sync::ignored_project;
    let mut projects = vec![Project {
        name: PROJECT.into(),
        folder: node.folder().to_string_lossy().into_owned(),
        peers: vec![peer.addr()],
        paused,
        rules: String::new(),
    }];

    if enabled {
        projects.push(Project {
            name: ignored_project(PROJECT, "assets/models"),
            folder: node.path("assets/models").to_string_lossy().into_owned(),
            peers: vec![peer.addr()],
            paused,
            rules: String::new(),
        });
    }

    let refused = node.running().transfers.syncs().configure(projects).await;
    assert!(refused.is_empty(), "{refused:?}");
}

fn ignored_status(node: &Node) -> ProjectStatus {
    let name = beep_proto::node::sync::ignored_project(PROJECT, "assets/models");
    node.running()
        .transfers
        .syncs()
        .status()
        .into_iter()
        .find(|s| s.name == name)
        .unwrap()
}

async fn ignored_pair() -> (Node, Node) {
    let (a, b) = synced_repo().await;
    commit(&a, ".gitignore", "assets/models/\n.env\n");
    in_step(&a, &b).await;
    ignored_config(&a, &b, true, false).await;
    ignored_config(&b, &a, true, false).await;
    (a, b)
}

#[tokio::test(flavor = "multi_thread")]
async fn selected_ignored_folders_sync_while_git_histories_differ_and_other_edits_stay_local() {
    let (a, b) = synced_repo().await;
    commit(&a, ".gitignore", "assets/models/\n.env\n");
    in_step(&a, &b).await;
    a.pause(&b, true).await;
    b.pause(&a, true).await;
    let ours = commit(&a, "code.txt", "ours\n");
    let theirs = commit(&b, "code.txt", "theirs\n");
    fs::write(a.path("README.md"), "uncommitted source\n").unwrap();
    fs::write(a.path(".env"), "LOCAL_SETTING=fixture\n").unwrap();
    ignored_config(&a, &b, true, false).await;
    ignored_config(&b, &a, true, false).await;
    let model = "assets/models/scene.blend";
    write(&a.path(model), b"model\0one");
    eventually("ignored binary data to cross differing commits", || {
        read(&b.path(model)).as_deref() == Some(b"model\0one")
    })
    .await;
    assert_eq!(head(&a).as_deref(), Some(ours.as_str()));
    assert_eq!(head(&b).as_deref(), Some(theirs.as_str()));
    assert!(!b.path(".env").exists());
    assert_ne!(read(&b.path("README.md")), read(&a.path("README.md")));
    write(&b.path(model), b"model\0two");
    eventually("ignored content to sync back", || {
        read(&a.path(model)).as_deref() == Some(b"model\0two")
    })
    .await;
    fs::remove_file(b.path(model)).unwrap();
    eventually("ignored file deletion to sync", || !a.path(model).exists()).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn disabling_ignored_folder_sync_leaves_files_and_stops_transferring() {
    let (a, b) = ignored_pair().await;
    let model = "assets/models/scene.blend";
    write(&a.path(model), "original");
    eventually("first ignored model", || {
        read(&b.path(model)).as_deref() == Some(b"original")
    })
    .await;
    ignored_config(&a, &b, false, false).await;
    ignored_config(&b, &a, false, false).await;
    write(&a.path(model), "kept locally");
    // Rescans and a complete peer exchange are the synchronization barrier.
    a.running().transfers.syncs().rescan(PROJECT).await.unwrap();
    b.running().transfers.syncs().rescan(PROJECT).await.unwrap();
    assert_eq!(
        read(&b.path(model)).as_deref(),
        Some(b"original".as_slice())
    );
    assert_eq!(
        read(&a.path(model)).as_deref(),
        Some(b"kept locally".as_slice())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn ignored_folder_sync_cannot_overwrite_a_force_added_model_or_its_index() {
    let (a, b) = ignored_pair().await;
    let model = "assets/models/scene.blend";
    write(&a.path(model), "original");
    eventually("first ignored model", || {
        read(&b.path(model)).as_deref() == Some(b"original")
    })
    .await;
    // Sync holds Git's index lock while it writes a batch; staging waits for
    // it to finish, as a user would.
    eventually("the ignored folder to finish writing", || {
        let status = ignored_status(&b);
        status.state == State::Idle && status.pending == 0
    })
    .await;
    git(&b.folder(), &["add", "-f", model]);
    let index = read(&b.path(".git/index"));
    write(&a.path(model), "peer edit");
    eventually("force-added content to block ignored-folder sync", || {
        ignored_status(&b)
            .error
            .as_deref()
            .is_some_and(|e| e.contains("tracked or staged"))
    })
    .await;
    assert_eq!(
        read(&b.path(model)).as_deref(),
        Some(b"original".as_slice())
    );
    assert_eq!(read(&b.path(".git/index")), index);
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_ignored_model_edits_keep_both_versions_and_can_be_resolved() {
    let (a, b) = ignored_pair().await;
    let model = "assets/models/scene.blend";
    write(&a.path(model), "baseline");
    eventually("model baseline", || {
        read(&b.path(model)).as_deref() == Some(b"baseline")
            && ignored_status(&b).files == 1
            && ignored_status(&b).pending == 0
    })
    .await;
    ignored_config(&a, &b, true, true).await;
    ignored_config(&b, &a, true, true).await;
    eventually(
        "ignored-folder sync to pause before concurrent edits",
        || ignored_status(&a).state == State::Paused && ignored_status(&b).state == State::Paused,
    )
    .await;
    write(&a.path(model), "local model");
    write(&b.path(model), "peer model");
    ignored_config(&a, &b, true, false).await;
    ignored_config(&b, &a, true, false).await;
    let both = |node: &Node| {
        let contents: Vec<_> = files(&node.path("assets/models"))
            .iter()
            .filter_map(|path| read(&node.path("assets/models").join(path)))
            .collect();
        !ignored_status(node).conflicts.is_empty()
            && contents.iter().any(|v| v == b"local model")
            && contents.iter().any(|v| v == b"peer model")
    };
    eventually("both ignored model versions on both machines", || {
        both(&a) && both(&b)
    })
    .await;
    let name = beep_proto::node::sync::ignored_project(PROJECT, "assets/models");
    timeout(
        LIMIT,
        a.running().transfers.syncs().have_all_from(
            &name,
            &b.addr().id,
            tokio::time::Instant::now(),
            |_| {},
        ),
    )
    .await
    .unwrap()
    .unwrap();
    let conflict = ignored_status(&a).conflicts[0].clone();
    a.running()
        .transfers
        .syncs()
        .resolve(&name, &conflict, Keep::Original)
        .await
        .unwrap();
    eventually("resolved ignored conflict to propagate", || {
        ignored_status(&a).conflicts.is_empty() && ignored_status(&b).conflicts.is_empty()
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ignored_files_wait_during_git_recovery_and_resume_after_review() {
    let (a, b) = ignored_pair().await;
    let model = "assets/models/scene.blend";
    write(&a.path(model), b"original model");
    eventually("the original ignored model", || {
        read(&b.path(model)).as_deref() == Some(b"original model")
    })
    .await;
    write(&b.path(".git/beep-recovery/pending.json"), "{}");
    write(&a.path(model), b"updated model");
    eventually("ignored sync to wait for recovery", || {
        ignored_status(&b)
            .error
            .as_deref()
            .is_some_and(|e| e.contains("Git recovery"))
    })
    .await;
    assert_eq!(
        read(&b.path(model)).as_deref(),
        Some(b"original model".as_slice())
    );
    fs::remove_file(b.path(".git/beep-recovery/pending.json")).unwrap();
    b.running().transfers.syncs().rescan(PROJECT).await.unwrap();
    eventually(
        "ignored sync to resume after the pending recovery is reviewed",
        || read(&b.path(model)).as_deref() == Some(b"updated model"),
    )
    .await;
}
