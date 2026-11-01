//! Two real nodes in this process sync one project folder node to node:
//! files go both ways, build output stays, deletes and conflicts carry
//! over, and a node that restarts mid-sync finishes without conflicts.
//!
//! The nodes react to their file watchers and to each other on their own
//! tasks, with nothing to wait on from outside, so the tests poll the
//! folders with a time limit.

use beep_node::{files::Folders, transfer::Transfers};
use beep_proto::node::sync::{Project, Rule, State};
use iroh::{Endpoint, EndpointAddr, SecretKey, endpoint::presets};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};
use tempfile::TempDir;
use tokio::time::{sleep, timeout};

const PROJECT: &str = "game";
const LIMIT: Duration = Duration::from_secs(60);
const MARKER: &str = ".beep-sync";

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
        let transfers = Transfers::start(&endpoint, self.state.path(), folders)
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
        let refused = self
            .running()
            .transfers
            .syncs()
            .configure(vec![Project {
                name: PROJECT.into(),
                folder: self.folder().to_string_lossy().into_owned(),
                peers: vec![peer.addr()],
            }])
            .await;
        assert!(refused.is_empty(), "{refused:?}");
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
                        .into_owned(),
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
async fn a_folder_syncs_both_ways_and_build_output_stays() {
    let (a, b) = pair().await;
    write(&a.path("Cargo.toml"), "[package]\nname = \"game\"\n");
    write(&a.path("src/main.rs"), "fn main() {}\n");
    write(&a.path("assets/big.bin"), data(300_000, 1));
    write(&a.path("target/debug/game"), data(1000, 2));
    fs::set_permissions(
        a.path("src/main.rs"),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    set_mtime(&a.path("assets/big.bin"), 1_700_000_000);

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
    let mode = std::os::unix::fs::PermissionsExt::mode(
        &fs::metadata(b.path("src/main.rs")).unwrap().permissions(),
    );
    assert_ne!(mode & 0o100, 0, "the executable bit carries over");

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
}

#[tokio::test(flavor = "multi_thread")]
async fn a_node_restarting_mid_sync_finishes_without_conflicts() {
    let (a, mut b) = pair().await;
    let count = 200;
    for n in 0..count {
        write(
            &a.path(&format!("frames/{n:04}.bin")),
            data(20_000, n as u8),
        );
    }
    eventually("the first files to arrive", || {
        b.path("frames").exists() && !files(&b.path("frames")).is_empty()
    })
    .await;
    b.stop().await;
    let arrived = files(&b.folder()).len();
    // A change made while it is down comes along too.
    write(&a.path("frames/0000.bin"), "changed while away");
    b.start().await;
    a.sync_with(&b).await;
    b.sync_with(&a).await;

    eventually("every file to arrive", || {
        let names = files(&b.folder());
        names.len() == count
            && read(&b.path("frames/0000.bin")).as_deref() == Some(b"changed while away".as_slice())
    })
    .await;
    for n in 1..count {
        let path = format!("frames/{n:04}.bin");
        assert_eq!(read(&b.path(&path)), Some(data(20_000, n as u8)), "{path}");
    }
    assert!(arrived <= count);
    assert!(
        fs::read_dir(b.folder()).unwrap().all(|e| {
            let name = e.unwrap().file_name();
            name == MARKER || !name.to_string_lossy().starts_with(".beep-")
        }),
        "no partial folder is left"
    );
    assert!(
        b.running().transfers.syncs().status()[0]
            .conflicts
            .is_empty()
    );
    assert_eq!(files(&a.folder()), files(&b.folder()));
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
        }])
        .await;
    assert!(refused.is_empty(), "{refused:?}");
    eventually("the pull to end", || syncs.pulls_served() == 0).await;
    drop(b);
}
