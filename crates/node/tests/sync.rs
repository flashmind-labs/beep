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
