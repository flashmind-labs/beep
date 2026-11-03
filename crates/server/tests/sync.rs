//! Two real nodes in this process sync a project folder while the app
//! drives them through the server: one turned on from the app, the other
//! given its list when it connects. Build output stays, the app sees how
//! each machine stands and what sync leaves out, a folder outside the
//! shared folders is refused, and pinning stops it everywhere.
//!
//! The nodes sync on their own tasks, with nothing to wait on from
//! outside, so the tests poll with a time limit.

use async_trait::async_trait;
use beep_node::{
    files::Folders,
    link::{Offers, Protocol},
    sync::Syncs,
    syncthing::Syncthing,
    transfer::Transfers,
};
use beep_server::{
    app::{App, ServerTools},
    client::{Client, Node},
    models::ModelChoices,
    nodes::{self, Online},
    protocol::{
        ApiError, ConflictKeep, DeviceKind, GetProjectSyncDetail, GetSyncSkipped, ListProjectSync,
        PauseProjectSync, PinProject, ProjectSync, ProjectSyncDetail, RemoveDevice, RemoveProject,
        ResolveSyncConflict, SetProjectSync, SkipRule, SyncStatus,
    },
    push::Pusher,
    store::{NewKey, SyncRow, TestDb},
    transport,
    user::User,
};
use flashmind::types::{
    AgentLlmConfig, CompletionRequest, CompletionStream, LlmProvider, Provider,
};
use iroh::{Endpoint, SecretKey, endpoint::presets, protocol::Router};
use std::{fs, future::Future, path::PathBuf, sync::Arc, time::Duration};
use tempfile::TempDir;
use tokio::{
    task::JoinHandle,
    time::{sleep, timeout},
};

const PROJECT: &str = "game";
const LIMIT: Duration = Duration::from_secs(60);

/// The sync tests never run a model turn.
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
        unreachable!("the sync tests make no model calls")
    }
}

async fn endpoint(key: SecretKey) -> Endpoint {
    Endpoint::builder(presets::N0)
        .secret_key(key)
        .bind()
        .await
        .expect("a test endpoint binds")
}

struct Server {
    _db: TestDb,
    app: Arc<App>,
    router: Router,
    user: Arc<User>,
    phone: Client,
    _phone_node: Node,
}

async fn server() -> Server {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = SecretKey::generate();
    let model = "openai:test".parse().unwrap();
    let models = ModelChoices::resolve(&NoModel, model, Vec::new()).await;
    let app = App::new(
        store.clone(),
        Arc::new(NoModel),
        AgentLlmConfig::new("openai:test".parse().unwrap()),
        ServerTools::default(),
        models,
    );
    let server_endpoint = endpoint(SecretKey::generate()).await;
    let pusher = Pusher::with_retry_waits(
        server_endpoint.clone(),
        Arc::new(store),
        None,
        vec![Duration::from_millis(200)],
    );
    let server_id = server_endpoint.id();
    let router = Router::builder(server_endpoint)
        .accept(
            nodes::NODE_ALPN,
            nodes::NodeHandler::new(app.clone(), server_id),
        )
        .accept(
            transport::ALPN,
            transport::BeepHandler::new(app.clone(), pusher),
        )
        .spawn();
    let user = app
        .sign_in(&alice.public())
        .await
        .unwrap()
        .expect("a new key is never revoked")
        .user;
    let phone_node = Node::bind(alice, router.endpoint().addr(), Arc::new(|_| true))
        .await
        .expect("a phone binds");
    let phone = phone_node.connect().await.expect("a phone connects");
    Server {
        _db: db,
        app,
        router,
        user,
        phone,
        _phone_node: phone_node,
    }
}

/// A node sharing its own temporary folder, which holds the project.
struct TestNode {
    id: String,
    online: Online,
    syncs: Syncs,
    shared: TempDir,
    _state: TempDir,
    _task: JoinHandle<anyhow::Result<()>>,
}

impl TestNode {
    fn folder(&self) -> PathBuf {
        self.shared
            .path()
            .join(PROJECT)
            .canonicalize()
            .expect("the project folder exists")
    }

    fn shown(&self) -> String {
        self.folder().to_string_lossy().into_owned()
    }
}

impl Server {
    /// Links a machine, and runs `before` once its key is linked and
    /// before it connects.
    async fn node(&self, name: &'static str, before: impl AsyncFnOnce(&str, &str)) -> TestNode {
        self.node_with(name, true, before).await
    }

    /// As `node`; without `controls` it is a beep-node from before sync
    /// could pause or settle conflicts.
    async fn node_with(
        &self,
        name: &'static str,
        controls: bool,
        before: impl AsyncFnOnce(&str, &str),
    ) -> TestNode {
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
        fs::create_dir(shared.path().join(PROJECT)).unwrap();
        let state = TempDir::new().expect("a temp folder");
        let id = key.public().to_string();
        let folder = shared
            .path()
            .join(PROJECT)
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        before(&id, &folder).await;
        let node = endpoint(key).await;
        let folders = Folders::new(&[shared.path().to_owned()]);
        // No Syncthing config: a marker in the shared folder is what says
        // Syncthing keeps the project in step.
        let syncthing = Syncthing::at(Vec::new(), Some(shared.path().to_owned()));
        let transfers = Transfers::start_with(&node, state.path(), folders.clone(), syncthing)
            .await
            .expect("the transfers start");
        let syncs = transfers.syncs();
        let mut extra = vec![
            Arc::new(transfers.syncs()) as Arc<dyn Protocol>,
            Arc::new(transfers) as Arc<dyn Protocol>,
            Arc::new(folders) as Arc<dyn Protocol>,
        ];
        if controls {
            extra.push(Arc::new(syncs.controls()));
        }
        let offers = Offers {
            agents: Arc::default(),
            extra,
            provision: None,
            tools: Default::default(),
        };
        let addr = self.router.endpoint().addr();
        let task =
            tokio::spawn(async move { beep_node::link::connect(&node, addr, name, &offers).await });
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
        assert!(online.offers(nodes::SYNC), "{:?}", online.offers);
        TestNode {
            id,
            online,
            syncs,
            shared,
            _state: state,
            _task: task,
        }
    }

    async fn set(&self, node: &TestNode, folder: &str, on: bool) -> Result<ProjectSync, ApiError> {
        let reply = self
            .phone
            .call(&SetProjectSync {
                project: PROJECT.into(),
                machine_id: node.id.clone(),
                folder: Some(folder.to_owned()),
                on,
            })
            .await;
        reply.map_err(|e| {
            e.downcast::<ApiError>()
                .expect("the server answers with an ApiError")
        })
    }
}

impl Server {
    async fn pause(&self, paused: bool) -> ProjectSyncDetail {
        self.phone
            .call(&PauseProjectSync {
                project: PROJECT.into(),
                paused,
            })
            .await
            .unwrap()
    }

    async fn detail(&self) -> ProjectSyncDetail {
        self.phone
            .call(&GetProjectSyncDetail {
                project: PROJECT.into(),
            })
            .await
            .unwrap()
    }

    async fn resolve(&self, node: &TestNode, path: &str) -> ApiError {
        let refused = self
            .phone
            .call(&ResolveSyncConflict {
                project: PROJECT.into(),
                machine_id: node.id.clone(),
                path: path.into(),
                keep: ConflictKeep::Copy {},
            })
            .await
            .expect_err("refused");
        refused
            .downcast::<ApiError>()
            .expect("the server answers with an ApiError")
    }
}

/// Waits until `check` holds, looking again every few milliseconds.
async fn eventually<F: Future<Output = bool>>(what: &str, mut check: impl FnMut() -> F) {
    let waited = timeout(LIMIT, async {
        while !check().await {
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await;
    assert!(waited.is_ok(), "timed out waiting for {what}");
}

fn write(path: PathBuf, data: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, data).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_app_turns_sync_on_and_sees_how_each_machine_stands() {
    let server = server().await;
    let a = server.node("render box", async |_, _| {}).await;
    let on = server.set(&a, &a.shown(), true).await.unwrap();
    assert_eq!(on.project, PROJECT);
    assert!(!on.pinned);
    assert_eq!(on.machines.len(), 1);
    assert_eq!(on.machines[0].machine_name, "render box");
    assert_eq!(on.machines[0].folder, a.shown());

    // The second machine is set up while it is away, and gets its list,
    // and the first its new peer, when it connects.
    let user = server.user.clone();
    let b = server
        .node("laptop", async |id, folder| {
            user.set_project_sync(&SyncRow {
                project: PROJECT.into(),
                endpoint: id.to_owned(),
                folder: folder.to_owned(),
            })
            .await
            .unwrap();
        })
        .await;

    write(
        a.folder().join("Cargo.toml"),
        "[package]\nname = \"game\"\n",
    );
    write(a.folder().join("src/main.rs"), "fn main() {}\n");
    write(a.folder().join("target/debug/game"), "built");
    eventually("the files to reach the laptop", async || {
        fs::read_to_string(b.folder().join("src/main.rs")).is_ok()
            && fs::read_to_string(b.folder().join("Cargo.toml")).is_ok()
    })
    .await;
    assert!(!b.folder().join("target").exists());

    eventually("both machines in sync", async || {
        let listed = server.phone.call(&ListProjectSync {}).await.unwrap();
        listed.len() == 1
            && listed[0].machines.len() == 2
            && listed[0]
                .machines
                .iter()
                .all(|machine| machine.status == SyncStatus::InSync {} && machine.files == 2)
    })
    .await;

    let skipped = server
        .phone
        .call(&GetSyncSkipped {
            project: PROJECT.into(),
            machine_id: a.id.clone(),
        })
        .await
        .unwrap();
    let target = skipped
        .paths
        .iter()
        .find(|path| path.path == "target")
        .expect("target is listed as left out");
    assert!(target.folder);
    assert_eq!(
        target.rule,
        SkipRule::Default {
            marker: "Cargo.toml".into()
        }
    );

    // A folder outside the shared folders is refused, and the machine
    // keeps the folder it had.
    let outside = TempDir::new().unwrap();
    let refused = server
        .set(&b, &outside.path().to_string_lossy(), true)
        .await
        .unwrap_err();
    assert!(
        matches!(refused, ApiError::BadRequest { .. }),
        "{refused:?}"
    );
    let listed = server.phone.call(&ListProjectSync {}).await.unwrap();
    let folders: Vec<_> = listed[0].machines.iter().map(|m| &m.folder).collect();
    assert!(folders.contains(&&b.shown()), "{folders:?}");
    assert_eq!(b.online.sync_status().await.unwrap().len(), 1);

    // Pinned, it stops on both machines and cannot be turned on.
    let pinned = server
        .phone
        .call(&PinProject {
            project: PROJECT.into(),
            pinned: true,
        })
        .await
        .unwrap();
    assert!(pinned.pinned);
    assert!(pinned.machines.is_empty());
    assert!(a.online.sync_status().await.unwrap().is_empty());
    assert!(b.online.sync_status().await.unwrap().is_empty());
    let refused = server.set(&a, &a.shown(), true).await.unwrap_err();
    assert!(
        matches!(refused, ApiError::BadRequest { .. }),
        "{refused:?}"
    );
    assert!(a.folder().join("src/main.rs").exists(), "the files stay");

    let unpinned = server
        .phone
        .call(&PinProject {
            project: PROJECT.into(),
            pinned: false,
        })
        .await
        .unwrap();
    assert!(!unpinned.pinned);
    server.set(&a, &a.shown(), true).await.unwrap();
    assert_eq!(a.online.sync_status().await.unwrap().len(), 1);

    // Off, the machine stops and its files stay.
    let off = server.set(&a, &a.shown(), false).await.unwrap();
    assert!(off.machines.is_empty());
    assert!(a.online.sync_status().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn pausing_holds_every_machine_and_an_older_node_stops_instead() {
    let server = server().await;
    let a = server.node("render box", async |_, _| {}).await;
    let old = server.node_with("old laptop", false, async |_, _| {}).await;
    server.set(&a, &a.shown(), true).await.unwrap();
    server.set(&old, &old.shown(), true).await.unwrap();
    write(a.folder().join("notes.txt"), "hello");
    eventually("the file to reach the older node", async || {
        fs::read_to_string(old.folder().join("notes.txt")).is_ok()
    })
    .await;

    let detail = server.detail().await;
    assert!(!detail.paused);
    let can: Vec<_> = detail
        .machines
        .iter()
        .map(|m| (m.sync.machine_name.as_str(), m.can_resolve))
        .collect();
    assert!(can.contains(&("render box", true)), "{can:?}");
    assert!(can.contains(&("old laptop", false)), "{can:?}");

    let paused = server.pause(true).await;
    assert!(paused.paused);
    // The newer node keeps the project and holds it; the older one stops.
    assert_eq!(a.online.sync_status().await.unwrap().len(), 1);
    assert!(old.online.sync_status().await.unwrap().is_empty());
    eventually("both machines to show paused", async || {
        let detail = server.detail().await;
        detail.machines.len() == 2
            && detail
                .machines
                .iter()
                .all(|m| m.paused && m.sync.status == SyncStatus::InSync {})
    })
    .await;
    let refused = server.resolve(&a, "notes (conflict 1a2b3c4d).txt").await;
    assert!(
        matches!(&refused, ApiError::BadRequest { message } if message.contains("paused")),
        "{refused:?}"
    );
    let refused = server.resolve(&old, "notes (conflict 1a2b3c4d).txt").await;
    assert!(
        matches!(&refused, ApiError::BadRequest { message } if message.contains("too old")),
        "{refused:?}"
    );

    let resumed = server.pause(false).await;
    assert!(!resumed.paused);
    eventually("the newer node to resume", async || {
        !a.online.sync_status().await.unwrap()[0].paused
    })
    .await;
    assert_eq!(old.online.sync_status().await.unwrap().len(), 1);
    write(a.folder().join("later.txt"), "after");
    eventually("a change after the resume to arrive", async || {
        fs::read_to_string(old.folder().join("later.txt")).is_ok()
    })
    .await;
    // The node answers for a path that is no conflict copy.
    let refused = server.resolve(&a, "notes.txt").await;
    assert!(
        matches!(&refused, ApiError::BadRequest { message } if message.contains("not a conflict copy")),
        "{refused:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removed_machine_stops_being_a_peer() {
    let server = server().await;
    let a = server.node("render box", async |_, _| {}).await;
    let b = server.node("laptop", async |_, _| {}).await;
    server.set(&a, &a.shown(), true).await.unwrap();
    server.set(&b, &b.shown(), true).await.unwrap();
    let b_key = b.online.connection.remote_id();
    assert_eq!(a.syncs.peers(PROJECT), [b_key]);

    server
        .phone
        .call(&RemoveDevice { id: b.id.clone() })
        .await
        .unwrap();
    assert!(a.syncs.peers(PROJECT).is_empty());
    let listed = server.phone.call(&ListProjectSync {}).await.unwrap();
    assert_eq!(listed[0].machines.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_a_project_stops_its_sync_everywhere_and_keeps_the_files() {
    let server = server().await;
    let a = server.node("render box", async |_, _| {}).await;
    let b = server.node("laptop", async |_, _| {}).await;
    server.set(&a, &a.shown(), true).await.unwrap();
    server.set(&b, &b.shown(), true).await.unwrap();
    write(a.folder().join("src/main.rs"), "fn main() {}\n");
    eventually("the file to reach the laptop", async || {
        fs::read_to_string(b.folder().join("src/main.rs")).is_ok()
    })
    .await;

    server
        .phone
        .call(&RemoveProject {
            project: PROJECT.into(),
        })
        .await
        .unwrap();
    // The server told both nodes before it answered.
    for node in [&a, &b] {
        assert!(node.syncs.status().is_empty(), "{:?}", node.syncs.status());
        assert!(node.syncs.peers(PROJECT).is_empty());
        assert_eq!(
            fs::read_to_string(node.folder().join("src/main.rs")).unwrap(),
            "fn main() {}\n"
        );
    }
    assert!(
        server
            .phone
            .call(&ListProjectSync {})
            .await
            .unwrap()
            .is_empty()
    );
    let again = server
        .phone
        .call(&RemoveProject {
            project: PROJECT.into(),
        })
        .await
        .unwrap_err()
        .downcast::<ApiError>()
        .expect("the server answers with an ApiError");
    assert_eq!(again, ApiError::NotFound {});
}

#[tokio::test(flavor = "multi_thread")]
async fn turning_sync_on_where_syncthing_syncs_is_refused() {
    let server = server().await;
    let a = server.node("render box", async |_, _| {}).await;
    fs::create_dir(a.shared.path().join(".stfolder")).unwrap();
    let refused = server.set(&a, &a.shown(), true).await.unwrap_err();
    let ApiError::BadRequest { message } = refused else {
        panic!("{refused:?}");
    };
    assert!(
        message.contains("Already synced by Syncthing ("),
        "{message}"
    );
    let listed = server.phone.call(&ListProjectSync {}).await.unwrap();
    assert!(listed.is_empty(), "{listed:?}");
}
