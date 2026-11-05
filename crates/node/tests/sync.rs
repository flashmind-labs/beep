//! Two real nodes in this process sync one project folder node to node:
//! files go both ways, build output stays, deletes and conflicts carry
//! over, and a node that restarts mid-sync finishes without conflicts.
//!
//! The nodes react to their file watchers and to each other on their own
//! tasks, with nothing to wait on from outside, so the tests poll the
//! folders with a time limit.

use beep_node::files::{Folders, syncthing::Syncthing, transfer::Transfers};
use beep_proto::node::sync::{Keep, Project, ProjectStatus, Refused, Rule, State};
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
async fn a_node_waits_for_all_a_peer_had_from_a_pull_that_went_through() {
    let (a, b) = pair().await;
    write(&a.path("src/main.rs"), "fn main() {}\n");
    eventually("the first node to list the file", || a.status().files >= 1).await;
    let a_id = a.addr().id;
    let since = tokio::time::Instant::now();
    let mut seen = 0;
    timeout(
        LIMIT,
        b.running()
            .transfers
            .syncs()
            .have_all_from(PROJECT, &a_id, since, |_| seen += 1),
    )
    .await
    .expect("in time")
    .expect("synced");
    assert!(seen >= 1);
    // The files are here when it answers, not on their way.
    assert_eq!(
        read(&b.path("src/main.rs")).as_deref(),
        Some(b"fn main() {}\n".as_slice())
    );
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
        let event = folder
            .wait_for(|folder| folder.event > since)
            .await
            .unwrap()
            .event;
        format!(r#"[{{"id":{event},"type":"FolderResumed"}}]"#)
    } else {
        let paused = folder.borrow().paused;
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
    let fsck = try_git(&dir, &["fsck", "--full", "--strict"]);
    assert!(
        fsck.status.success(),
        "{}",
        String::from_utf8_lossy(&fsck.stderr)
    );
    git(&dir, &["log", "--all", "--oneline"]);
    let names = files(&dir.join(".git"));
    assert!(
        !names.iter().any(|name| name.contains(" (conflict ")),
        "{names:?}"
    );
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
async fn a_commit_arrives_with_a_clean_status_both_ways() {
    let a = Node::new().await;
    let b = Node::new().await;
    new_repo(&a);
    write(&a.path(".gitignore"), "local.env\ntarget/built.o\n");
    write(&a.path("local.env"), "ignored, and still carried\n");
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
    eventually("the ignored file to arrive", || {
        read(&b.path("local.env")).as_deref() == Some(b"ignored, and still carried\n".as_slice())
    })
    .await;
    assert!(read(&b.path("target/built.o")).is_none());
    assert!(!b.path(".git/index.lock").exists());

    let there = commit(&a, "src/lib.rs", "pub fn a() {}\n");
    in_step(&a, &b).await;
    assert_eq!(head(&b).as_deref(), Some(there.as_str()));
    assert_eq!(
        git(&b.folder(), &["log", "--format=%s", "-1"]),
        "src/lib.rs"
    );

    // The other way, with another branch, a tag and packed refs.
    git(&b.folder(), &["tag", "v1"]);
    git(&b.folder(), &["branch", "side"]);
    git(&b.folder(), &["pack-refs", "--all"]);
    let back = commit(&b, "src/lib.rs", "pub fn b() {}\n");
    in_step(&a, &b).await;
    assert_eq!(head(&a).as_deref(), Some(back.as_str()));
    eventually("the tag and the branch to arrive", || {
        has_ref(&a, "refs/tags/v1") && has_ref(&a, "refs/heads/side")
    })
    .await;
    // The reflog says where a branch moved here, by sync as by anything.
    assert!(git(&a.folder(), &["reflog", "show", "main"]).contains("beep-sync"));

    // A branch switched and one deleted follow.
    git(&a.folder(), &["switch", "-q", "-c", "topic"]);
    git(&a.folder(), &["branch", "-D", "side"]);
    let topic = commit(&a, "topic.txt", "topic\n");
    in_step(&a, &b).await;
    assert_eq!(head(&b).as_deref(), Some(topic.as_str()));
    eventually("HEAD and the deleted branch to follow", || {
        git(&b.folder(), &["symbolic-ref", "HEAD"]) == "refs/heads/topic"
            && !has_ref(&b, "refs/heads/side")
    })
    .await;
    assert_sound(&b);
}

#[tokio::test(flavor = "multi_thread")]
async fn commits_on_both_sides_of_a_branch_stay_reachable() {
    let (a, b) = synced_repo().await;
    a.pause(&b, true).await;
    b.pause(&a, true).await;
    let ours = commit(&a, "ours.txt", "ours\n");
    let theirs = commit(&b, "theirs.txt", "theirs\n");
    a.pause(&b, false).await;
    b.pause(&a, false).await;

    let has_both = |node: &Node| {
        let all = git(&node.folder(), &["log", "--all", "--format=%H"]);
        all.contains(&ours) && all.contains(&theirs)
    };
    eventually("both commits and one branch tip on both machines", || {
        has_both(&a) && has_both(&b) && head(&a) == head(&b)
    })
    .await;
    let kept = |node: &Node| {
        git(
            &node.folder(),
            &[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/beep-conflicts",
            ],
        )
    };
    eventually("the losing side kept as a ref on both", || {
        !kept(&a).is_empty() && kept(&a) == kept(&b)
    })
    .await;
    assert_sound(&a);
    assert_sound(&b);
    eventually("the files of both commits on both", || {
        [&a, &b].iter().all(|node| {
            read(&node.path("ours.txt")).is_some() && read(&node.path("theirs.txt")).is_some()
        })
    })
    .await;
    eventually("the conflict to be shown", || {
        a.status()
            .conflicts
            .iter()
            .any(|path| path.starts_with(".git/refs/beep-conflicts/heads/main-"))
    })
    .await;

    // Kept by the user: the branch moves there, and the ref is gone.
    let copy = a.status().conflicts[0].clone();
    let listed = kept(&a);
    let lost = listed.split(' ').nth(1).unwrap().to_owned();
    a.running()
        .transfers
        .syncs()
        .resolve(PROJECT, &copy, Keep::Copy)
        .await
        .unwrap();
    eventually("the branch to move on both", || {
        head(&a).as_deref() == Some(lost.as_str())
            && head(&b).as_deref() == Some(lost.as_str())
            && kept(&a).is_empty()
            && kept(&b).is_empty()
    })
    .await;
    assert_sound(&a);
    assert_sound(&b);
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
async fn twin_checkouts_make_no_conflict_and_a_branch_behind_moves_forward() {
    let (a, b) = twins().await;
    git(&a.folder(), &["branch", "side"]);
    git(&b.folder(), &["branch", "side"]);
    git(&a.folder(), &["switch", "-q", "side"]);
    let ahead = commit(&a, "side.txt", "side\n");
    git(&a.folder(), &["switch", "-q", "main"]);
    a.sync_with(&b).await;
    b.sync_with(&a).await;
    let side = |node: &Node| git(&node.folder(), &["rev-parse", "refs/heads/side"]);
    let none_kept =
        |node: &Node| git(&node.folder(), &["for-each-ref", "refs/beep-conflicts"]).is_empty();
    eventually("the branch to be ahead on both, with nothing kept", || {
        side(&a) == ahead && side(&b) == ahead && none_kept(&a) && none_kept(&b)
    })
    .await;
    in_step(&a, &b).await;
    for name in files(&a.folder()) {
        assert!(!name.contains(" (conflict "), "{name}");
    }
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
async fn a_linked_worktree_stays_on_its_machine_and_its_branch_travels() {
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

    // A commit on that branch elsewhere waits here, as the worktree has it
    // checked out, and moves nothing from under it.
    git(&b.folder(), &["switch", "-q", "feat"]);
    let moved = commit(&b, "elsewhere.txt", "elsewhere\n");
    eventually("the status to say what waits", || {
        a.status()
            .error
            .as_deref()
            .is_some_and(|error| error.contains("linked worktree"))
    })
    .await;
    assert_eq!(git(&feat, &["rev-parse", "HEAD"]), tip);
    assert_ne!(moved, tip);
    assert!(git(&feat, &["status", "--porcelain"]).is_empty());
    assert_sound(&a);
}

#[tokio::test(flavor = "multi_thread")]
async fn gc_on_one_side_leaves_both_sound() {
    let a = Node::new().await;
    let b = Node::new().await;
    new_repo(&a);
    for n in 0..5 {
        commit(&a, &format!("{n}.txt"), &format!("{n}\n"));
    }
    a.sync_with(&b).await;
    b.sync_with(&a).await;
    in_step(&a, &b).await;
    let loose = |node: &Node| {
        files(&node.path(".git/objects"))
            .iter()
            .filter(|name| name.len() == 41)
            .count()
    };
    let before = loose(&b);
    assert!(before > 0);

    git(&a.folder(), &["gc", "-q", "--prune=now"]);
    assert_eq!(loose(&a), 0);
    assert_sound(&a);
    // Work goes on. The pack comes with it, and what it replaced stays on
    // the other side until its own gc.
    let next = commit(&a, "after.txt", "after gc\n");
    in_step(&a, &b).await;
    eventually("the pack to arrive", || {
        files(&b.path(".git/objects"))
            .iter()
            .any(|name| name.ends_with(".idx"))
    })
    .await;
    assert_sound(&b);
    assert_eq!(head(&b).as_deref(), Some(next.as_str()));
    assert!(loose(&b) >= before);
    // What the gc removed does not come back.
    assert!(loose(&a) < before);

    git(&b.folder(), &["gc", "-q", "--prune=now"]);
    let last = commit(&b, "last.txt", "after both\n");
    in_step(&a, &b).await;
    assert_eq!(head(&a).as_deref(), Some(last.as_str()));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_node_on_the_old_protocol_is_turned_away() {
    let (a, b) = synced_repo().await;
    let refs = git(&a.folder(), &["show-ref"]);
    for alpn in [&b"beep-sync/1"[..], b"beep-sync/2"] {
        let old = b.running().endpoint.connect(a.addr(), alpn).await;
        assert!(old.is_err(), "an old node is not served");
    }
    assert_sound(&a);
    assert_eq!(git(&a.folder(), &["show-ref"]), refs);
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
    eventually("the peer's files to arrive", || {
        read(&a.path("theirs.txt")).is_some() && read(&b.path("README.md")).is_some()
    })
    .await;
    assert_sound(&a);
    assert_eq!(git(&a.folder(), &["show-ref"]), refs);
    assert!(!b.path(".git").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn git_removed_on_one_machine_comes_back_and_stays_on_the_other() {
    let (a, b) = synced_repo().await;
    let there = head(&b);
    fs::remove_dir_all(a.path(".git")).unwrap();
    // Longer than a delete takes to settle and carry over.
    sleep(Duration::from_secs(8)).await;
    assert_eq!(head(&b), there);
    assert_sound(&b);
    in_step(&a, &b).await;
    assert_eq!(head(&a), there);
}

#[tokio::test(flavor = "multi_thread")]
async fn two_checkouts_at_other_commits_hold_until_they_agree() {
    let a = Node::new().await;
    let b = Node::new().await;
    new_repo(&a);
    new_repo(&b);
    let ours = commit(&a, "a.txt", "only on a\n");
    let theirs = commit(&b, "b.txt", "only on b\n");
    write(&a.path("same.txt"), "this side\n");
    write(&b.path("same.txt"), "that side\n");
    a.sync_with(&b).await;
    b.sync_with(&a).await;

    let held = |node: &Node| {
        node.status()
            .error
            .as_deref()
            .is_some_and(|error| error.contains("one commit"))
    };
    eventually("both to say why nothing syncs", || held(&a) && held(&b)).await;
    assert_eq!(head(&a).as_deref(), Some(ours.as_str()));
    assert_eq!(head(&b).as_deref(), Some(theirs.as_str()));
    assert_eq!(
        read(&a.path("same.txt")).as_deref(),
        Some(b"this side\n".as_slice())
    );
    assert_eq!(
        read(&b.path("same.txt")).as_deref(),
        Some(b"that side\n".as_slice())
    );
    assert!(read(&a.path("b.txt")).is_none());
    assert_sound(&a);
    assert_sound(&b);

    // The user brings them to one commit with git, and sync takes over.
    fs::remove_file(b.path("same.txt")).unwrap();
    git(
        &b.folder(),
        &["fetch", "-q", &a.folder().to_string_lossy(), "main"],
    );
    git(&b.folder(), &["switch", "-q", "--detach", "FETCH_HEAD"]);
    git(&b.folder(), &["branch", "-f", "main", "FETCH_HEAD"]);
    git(&b.folder(), &["switch", "-q", "main"]);
    eventually("the hold to lift", || {
        !held(&a) && !held(&b) && read(&b.path("same.txt")).is_some()
    })
    .await;
    assert_eq!(head(&b).as_deref(), Some(ours.as_str()));
    // The one that already was at that commit stays there, with no branch
    // kept for a losing side.
    assert_eq!(head(&a).as_deref(), Some(ours.as_str()));
    assert!(!git(&a.folder(), &["show-ref"]).contains("beep-conflicts"));
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
