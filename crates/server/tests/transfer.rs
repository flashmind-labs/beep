//! Two real nodes in this process send each other a file and a folder over
//! `transfer/1`: the server only brokers, the bytes go node to node over
//! iroh-blobs. A relative target is refused, a taken name keeps both, a
//! closed offer cannot be fetched again, and a cancelled transfer leaves
//! nothing in the target folder.

use async_trait::async_trait;
use beep_node::{
    files::Folders,
    link::{Offers, Protocol},
    transfer::Transfers,
};
use beep_server::{
    app::{App, ServerTools},
    models::ModelChoices,
    nodes::{self, Online, transfer::Progress},
    protocol::{DeviceKind, node::transfer::Stage},
    store::{NewKey, TestDb},
};
use flashmind::types::{
    AgentLlmConfig, CompletionRequest, CompletionStream, LlmProvider, Provider,
};
use iroh::{Endpoint, SecretKey, endpoint::presets, protocol::Router};
use std::{fs, path::Path, sync::Arc, time::Duration};
use tempfile::TempDir;
use tokio::{task::JoinHandle, time::timeout};
use tokio_util::sync::CancellationToken;

/// The transfer tests never run a model turn.
struct NoModel;

#[async_trait]
impl LlmProvider for NoModel {
    fn name(&self) -> &str {
        "none"
    }
    fn provider(&self) -> Provider {
        Provider::OpenAi
    }
    fn complete(&self, _request: CompletionRequest) -> CompletionStream {
        unreachable!("the transfer tests make no model calls")
    }
}

async fn endpoint(key: SecretKey) -> Endpoint {
    Endpoint::builder(presets::N0)
        .secret_key(key)
        .bind()
        .await
        .expect("a test endpoint binds")
}

/// A node sharing its own temporary folder, serving files and transfers.
struct TestNode {
    online: Online,
    shared: TempDir,
    _state: TempDir,
    _task: JoinHandle<anyhow::Result<()>>,
}

struct Server {
    _db: TestDb,
    app: Arc<App>,
    router: Router,
    user: Arc<beep_server::user::User>,
}

async fn server() -> Server {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = SecretKey::generate();
    let model = "openai:test".parse().unwrap();
    let models = ModelChoices::resolve(&NoModel, model, Vec::new(), &[alice.public()]).await;
    let app = App::new(
        store,
        Arc::new(NoModel),
        AgentLlmConfig::new("openai:test".parse().unwrap()),
        ServerTools::default(),
        models,
    );
    let server_endpoint = endpoint(SecretKey::generate()).await;
    let handler = nodes::NodeHandler::new(app.clone(), server_endpoint.id());
    let router = Router::builder(server_endpoint)
        .accept(nodes::NODE_ALPN, handler)
        .spawn();
    let user = app
        .sign_in(&alice.public())
        .await
        .unwrap()
        .expect("a new key is never revoked")
        .user;
    Server {
        _db: db,
        app,
        router,
        user,
    }
}

impl Server {
    async fn node(&self, name: &'static str) -> TestNode {
        let key = SecretKey::generate();
        self.user
            .link_key(NewKey {
                endpoint: key.public(),
                name,
                kind: DeviceKind::Desktop {},
            })
            .await
            .unwrap();
        let shared = TempDir::new().expect("a temp folder");
        let state = TempDir::new().expect("a temp folder");
        let node = endpoint(key.clone()).await;
        let folders = Folders::new(&[shared.path().to_owned()]);
        let transfers = Transfers::start(&node, state.path(), folders.clone())
            .await
            .expect("the transfers start");
        let offers = Offers {
            agents: Arc::default(),
            extra: vec![
                Arc::new(transfers) as Arc<dyn Protocol>,
                Arc::new(folders) as Arc<dyn Protocol>,
            ],
            provision: None,
        };
        let addr = self.router.endpoint().addr();
        let task =
            tokio::spawn(async move { beep_node::link::connect(&node, addr, name, &offers).await });
        let id = key.public().to_string();
        // Registering is the server's own business with no signal to wait
        // on, so this looks until it is there.
        let online = timeout(Duration::from_secs(20), async {
            loop {
                if let Some(online) = self.app.nodes.online(&id) {
                    return online;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the node came online");
        assert!(online.offers(nodes::TRANSFER), "{:?}", online.offers);
        TestNode {
            online,
            shared,
            _state: state,
            _task: task,
        }
    }
}

fn shown(path: &Path) -> String {
    path.canonicalize()
        .expect("exists")
        .to_string_lossy()
        .into_owned()
}

fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(dir)
        .expect("listed")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// Data no two files in a test share.
fn data(len: usize, seed: u8) -> Vec<u8> {
    (0..len).map(|n| (n % 251) as u8 ^ seed).collect()
}

fn ignore(_: Progress) {}

#[tokio::test]
async fn a_file_and_a_folder_go_node_to_node() {
    let server = server().await;
    let source = server.node("render box").await;
    let target = server.node("laptop").await;
    let peer = target.online.connection.remote_id();

    let render = data(300_000, 1);
    fs::write(source.shared.path().join("render.png"), &render).unwrap();
    fs::create_dir_all(source.shared.path().join("frames/deep")).unwrap();
    fs::write(
        source.shared.path().join("frames/0001.png"),
        data(20_000, 2),
    )
    .unwrap();
    fs::write(source.shared.path().join("frames/0002.png"), data(10, 3)).unwrap();
    fs::write(source.shared.path().join("frames/deep/notes.txt"), b"hi").unwrap();
    let inbox = target.shared.path().join("in");
    fs::create_dir_all(&inbox).unwrap();
    // A file already there keeps its name and its bytes.
    fs::write(inbox.join("render.png"), b"older").unwrap();
    let folder = shown(&inbox);

    let mut hashed = Vec::new();
    let offer = source
        .online
        .offer(
            &shown(&source.shared.path().join("render.png")),
            peer,
            &mut |p| hashed.push(p),
        )
        .await
        .unwrap();
    assert_eq!(offer.offered.size, 300_000);
    let mut fetched = Vec::new();
    let landed = target
        .online
        .fetch(
            &offer.offered,
            &folder,
            &mut |p| fetched.push(p),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(landed.path, format!("{folder}/render (2).png"));
    assert_eq!((landed.size, landed.files), (300_000, 1));
    assert_eq!(fs::read(inbox.join("render (2).png")).unwrap(), render);
    assert_eq!(fs::read(inbox.join("render.png")).unwrap(), b"older");
    assert_eq!(
        fetched.last(),
        Some(&Progress {
            done: 300_000,
            total: 300_000,
            stage: Stage::Writing,
        })
    );
    assert!(
        fetched.iter().any(|p| p.stage == Stage::Downloading),
        "{fetched:?}"
    );
    assert_eq!(
        hashed.last().map(|p| (p.done, p.stage)),
        Some((300_000, Stage::Hashing))
    );

    // The same content again lands under the next free name.
    let landed = target
        .online
        .fetch(
            &offer.offered,
            &folder,
            &mut ignore,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(landed.path, format!("{folder}/render (3).png"));
    fs::remove_file(&landed.path).unwrap();
    offer.close().await.unwrap();

    // Once an offer is closed, the target cannot fetch it.
    fs::write(source.shared.path().join("late.png"), data(5_000, 4)).unwrap();
    let late = source
        .online
        .offer(
            &shown(&source.shared.path().join("late.png")),
            peer,
            &mut ignore,
        )
        .await
        .unwrap();
    let closed = late.offered.clone();
    late.close().await.unwrap();
    let refused = target
        .online
        .fetch(&closed, &folder, &mut ignore, &CancellationToken::new())
        .await
        .unwrap_err();
    assert!(format!("{refused:#}").contains("laptop"), "{refused:#}");
    assert_eq!(entries(&inbox), ["render (2).png", "render.png"]);

    let offer = source
        .online
        .offer(
            &shown(&source.shared.path().join("frames")),
            peer,
            &mut ignore,
        )
        .await
        .unwrap();
    assert_eq!((offer.offered.files, offer.offered.size), (3, 20_012));
    let landed = target
        .online
        .fetch(
            &offer.offered,
            &folder,
            &mut ignore,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(landed.path, format!("{folder}/frames"));
    assert_eq!(landed.files, 3);
    assert_eq!(
        fs::read(inbox.join("frames/0001.png")).unwrap(),
        data(20_000, 2)
    );
    assert_eq!(
        fs::read(inbox.join("frames/deep/notes.txt")).unwrap(),
        b"hi"
    );
    // The same folder again keeps both.
    let landed = target
        .online
        .fetch(
            &offer.offered,
            &folder,
            &mut ignore,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(landed.path, format!("{folder}/frames (2)"));
    assert_eq!(
        entries(&inbox),
        ["frames", "frames (2)", "render (2).png", "render.png"]
    );
    server.router.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_relative_or_outside_target_is_refused() {
    let server = server().await;
    let source = server.node("render box").await;
    let target = server.node("laptop").await;
    fs::write(source.shared.path().join("a.txt"), b"a").unwrap();
    fs::create_dir_all(target.shared.path().join("in")).unwrap();
    let offer = source
        .online
        .offer(
            &shown(&source.shared.path().join("a.txt")),
            target.online.connection.remote_id(),
            &mut ignore,
        )
        .await
        .unwrap();
    let relative = target
        .online
        .fetch(&offer.offered, "in", &mut ignore, &CancellationToken::new())
        .await
        .unwrap_err();
    assert!(
        format!("{relative:#}").contains("not an absolute path"),
        "{relative:#}"
    );
    let outside = TempDir::new().unwrap();
    let refused = target
        .online
        .fetch(
            &offer.offered,
            &shown(outside.path()),
            &mut ignore,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert!(
        format!("{refused:#}").contains("outside the shared folders"),
        "{refused:#}"
    );
    assert!(entries(outside.path()).is_empty());
    // Nor does the source offer what it does not share.
    let secret = outside.path().join("secret.txt");
    fs::write(&secret, b"no").unwrap();
    let refused = source
        .online
        .offer(
            &shown(&secret),
            target.online.connection.remote_id(),
            &mut ignore,
        )
        .await
        .err()
        .expect("refused");
    assert!(
        format!("{refused:#}").contains("outside the shared folders"),
        "{refused:#}"
    );
    server.router.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_cancelled_transfer_leaves_nothing_behind() {
    let server = server().await;
    let source = server.node("render box").await;
    let target = server.node("laptop").await;
    fs::write(source.shared.path().join("big.bin"), data(64 << 20, 9)).unwrap();
    let inbox = target.shared.path().join("in");
    fs::create_dir_all(&inbox).unwrap();
    let offer = source
        .online
        .offer(
            &shown(&source.shared.path().join("big.bin")),
            target.online.connection.remote_id(),
            &mut ignore,
        )
        .await
        .unwrap();
    // Cancelled as soon as the first bytes arrive.
    let cancel = CancellationToken::new();
    let mut on_progress = |p: Progress| {
        if p.done > 0 && p.done < p.total {
            cancel.cancel();
        }
    };
    let cancelled = target
        .online
        .fetch(&offer.offered, &shown(&inbox), &mut on_progress, &cancel)
        .await
        .unwrap_err();
    assert!(
        format!("{cancelled:#}").contains("cancelled"),
        "{cancelled:#}"
    );
    // The node answers only once it removed what it wrote.
    assert!(entries(&inbox).is_empty(), "{:?}", entries(&inbox));
    server.router.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_target_folder_swapped_for_a_link_mid_transfer_keeps_writes_inside() {
    let server = server().await;
    let source = server.node("render box").await;
    let target = server.node("laptop").await;
    fs::write(source.shared.path().join("big.bin"), data(32 << 20, 5)).unwrap();
    let inbox = target.shared.path().join("in");
    fs::create_dir_all(&inbox).unwrap();
    let folder = shown(&inbox);
    let moved = target.shared.path().join("moved");
    let outside = TempDir::new().unwrap();
    let offer = source
        .online
        .offer(
            &shown(&source.shared.path().join("big.bin")),
            target.online.connection.remote_id(),
            &mut ignore,
        )
        .await
        .unwrap();
    // Once bytes arrive, the folder is moved away and a link to a folder
    // outside the shared ones takes its place.
    let mut swapped = false;
    let mut swap = |p: Progress| {
        if !swapped && p.done > 0 {
            swapped = true;
            fs::rename(&inbox, &moved).unwrap();
            std::os::unix::fs::symlink(outside.path(), &inbox).unwrap();
        }
    };
    let landed = target
        .online
        .fetch(
            &offer.offered,
            &folder,
            &mut swap,
            &CancellationToken::new(),
        )
        .await;
    assert!(swapped, "the transfer reported progress");
    assert!(
        entries(outside.path()).is_empty(),
        "nothing lands outside: {:?} ({landed:?})",
        entries(outside.path())
    );
    let landed = landed.unwrap();
    assert!(landed.path.ends_with("/big.bin"), "{landed:?}");
    assert_eq!(entries(&moved), ["big.bin"]);
    server.router.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_folder_keeps_executable_bits_and_empty_folders() {
    use std::os::unix::fs::PermissionsExt;
    let server = server().await;
    let source = server.node("render box").await;
    let target = server.node("laptop").await;
    let tools = source.shared.path().join("tools");
    fs::create_dir_all(tools.join("empty/inner")).unwrap();
    fs::write(tools.join("run.sh"), b"#!/bin/sh\necho hi\n").unwrap();
    fs::set_permissions(tools.join("run.sh"), fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(tools.join("notes.txt"), b"plain").unwrap();
    fs::set_permissions(tools.join("notes.txt"), fs::Permissions::from_mode(0o644)).unwrap();
    let inbox = target.shared.path().join("in");
    fs::create_dir_all(&inbox).unwrap();
    let peer = target.online.connection.remote_id();
    let offer = source
        .online
        .offer(&shown(&tools), peer, &mut ignore)
        .await
        .unwrap();
    assert_eq!(offer.offered.files, 2);
    target
        .online
        .fetch(
            &offer.offered,
            &shown(&inbox),
            &mut ignore,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o111;
    assert_ne!(mode(&inbox.join("tools/run.sh")), 0, "still executable");
    assert_eq!(mode(&inbox.join("tools/notes.txt")), 0);
    assert!(inbox.join("tools/empty/inner").is_dir());
    assert_eq!(
        entries(&inbox.join("tools")),
        ["empty", "notes.txt", "run.sh"]
    );

    // One executable file keeps its bit too.
    let offer = source
        .online
        .offer(&shown(&tools.join("run.sh")), peer, &mut ignore)
        .await
        .unwrap();
    let landed = target
        .online
        .fetch(
            &offer.offered,
            &shown(&inbox),
            &mut ignore,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_ne!(mode(Path::new(&landed.path)), 0);
    server.router.shutdown().await.unwrap();
}
