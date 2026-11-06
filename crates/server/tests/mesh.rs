//! A desktop that imports a project on its own tells the server, so phones
//! and the project agent still see it.

use async_trait::async_trait;
use beep_server::{
    account::user::User,
    agent::models::ModelChoices,
    api::http,
    app::{App, ServerTools},
    protocol::{ApiError, DeviceKind, ListImportedProjects, MirrorImport, client::Client},
    push::Pulse,
    store::{NewKey, NewNode, TestDb},
};
use flashmind::types::{
    AgentLlmConfig, CompletionRequest, CompletionStream, LlmProvider, Provider,
};
use iroh::SecretKey;
use std::sync::Arc;

/// These tests never run a model turn.
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
        unreachable!("the mesh tests make no model calls")
    }
}

struct Harness {
    _db: TestDb,
    apps: http::Local,
    app: Arc<App>,
}

/// One signed-in account: its user row and a connected app.
struct Account {
    user: Arc<User>,
    phone: Client,
}

async fn harness() -> Harness {
    let db = TestDb::new().await.expect("test database");
    let app = App::new(
        db.store().await.expect("test store"),
        Arc::new(NoModel),
        AgentLlmConfig::new("openai:test".parse().expect("a valid model")),
        ServerTools::default(),
        ModelChoices::only("openai:test".parse().expect("a valid model")),
    );
    let apps = http::Local::start(app.clone(), &SecretKey::generate(), Pulse::manual())
        .await
        .expect("the apps' HTTP starts");
    Harness { _db: db, apps, app }
}

impl Harness {
    async fn account(&self) -> Account {
        let identity = SecretKey::generate();
        let user = self
            .app
            .sign_in(&identity.public())
            .await
            .expect("sign in")
            .expect("a new key is never revoked")
            .user;
        let phone = Client::new(self.apps.url(), identity).expect("an app client");
        Account { user, phone }
    }
}

impl Account {
    /// A machine with a key and no nodes row, as a desktop's own machine is.
    async fn key_only_machine(&self) -> String {
        let key = SecretKey::generate().public();
        self.user
            .link_key(NewKey {
                endpoint: key,
                name: "desk",
                kind: DeviceKind::Desktop {},
            })
            .await
            .expect("the machine links");
        key.to_string()
    }

    /// A machine whose beep-node has connected.
    async fn node_machine(&self) -> String {
        let key = SecretKey::generate().public();
        self.user
            .link_key(NewKey {
                endpoint: key,
                name: "build box",
                kind: DeviceKind::Desktop {},
            })
            .await
            .expect("the machine links");
        self.user
            .node_connected(NewNode {
                endpoint: key,
                name: "build box",
                version: "0.1.0",
            })
            .await
            .expect("the node connects");
        key.to_string()
    }

    async fn mirror(&self, request: &MirrorImport) -> Result<(), ApiError> {
        self.phone.call(request).await.map_err(|e| {
            e.downcast::<ApiError>()
                .expect("the server answers with an ApiError")
        })
    }

    async fn list(&self) -> Vec<beep_server::protocol::ImportedProject> {
        self.phone.call(&ListImportedProjects {}).await.unwrap()
    }
}

fn mirror(machine_id: &str) -> MirrorImport {
    MirrorImport {
        project: "org/widget".into(),
        machine_id: machine_id.into(),
        folder: "/srv/widget".into(),
        url: "https://example.com/org/widget".into(),
    }
}

fn bad_request(result: Result<(), ApiError>) -> String {
    match result {
        Err(ApiError::BadRequest { message }) => message,
        other => panic!("expected a bad request, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_mirrored_import_shows_in_the_servers_project_list() {
    let h = harness().await;
    let a = h.account().await;
    let machine = a.key_only_machine().await;
    a.mirror(&mirror(&machine)).await.unwrap();
    a.mirror(&mirror(&machine)).await.unwrap();

    let listed = a.list().await;
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0].project, "org/widget");
    assert_eq!(listed[0].machine_id, machine);
    assert_eq!(listed[0].machine_name, "desk");
    assert_eq!(listed[0].folder, "/srv/widget");
    assert_eq!(listed[0].url, "https://example.com/org/widget");
    assert!(!listed[0].cloned);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_mirrored_import_on_a_connected_node_shows_with_its_name() {
    let h = harness().await;
    let a = h.account().await;
    let machine = a.node_machine().await;
    a.mirror(&mirror(&machine)).await.unwrap();
    let listed = a.list().await;
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0].machine_name, "build box");
}

#[tokio::test(flavor = "multi_thread")]
async fn another_accounts_machine_is_refused_and_changes_nothing() {
    let h = harness().await;
    let x = h.account().await;
    let y = h.account().await;
    let machine = x.key_only_machine().await;

    assert_eq!(
        y.mirror(&mirror(&machine)).await.unwrap_err(),
        ApiError::NotFound {}
    );
    assert!(x.list().await.is_empty());
    assert!(y.list().await.is_empty());
    // The same call is accepted from the owner, so ownership was the reason.
    x.mirror(&mirror(&machine)).await.unwrap();
    assert_eq!(x.list().await.len(), 1);
    assert!(y.list().await.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_machine_is_not_found() {
    let h = harness().await;
    let a = h.account().await;
    for id in [SecretKey::generate().public().to_string(), "nope".into()] {
        assert_eq!(
            a.mirror(&mirror(&id)).await.unwrap_err(),
            ApiError::NotFound {}
        );
    }
    assert!(a.list().await.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn project_names_that_differ_in_case_make_one_row() {
    let h = harness().await;
    let a = h.account().await;
    let machine = a.key_only_machine().await;
    a.mirror(&mirror(&machine)).await.unwrap();
    a.mirror(&MirrorImport {
        project: "Org/Widget".into(),
        folder: "/srv/elsewhere".into(),
        ..mirror(&machine)
    })
    .await
    .unwrap();
    let listed = a.list().await;
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0].project, "org/widget");
    assert_eq!(listed[0].folder, "/srv/elsewhere");
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_input_is_refused_and_stores_nothing() {
    let h = harness().await;
    let a = h.account().await;
    let machine = a.key_only_machine().await;
    let long = "a".repeat(3000);
    let cases = [
        MirrorImport {
            project: "widget".into(),
            ..mirror(&machine)
        },
        MirrorImport {
            project: "org/wid get".into(),
            ..mirror(&machine)
        },
        MirrorImport {
            folder: "srv/widget".into(),
            ..mirror(&machine)
        },
        MirrorImport {
            folder: "/srv/../etc".into(),
            ..mirror(&machine)
        },
        MirrorImport {
            folder: "".into(),
            ..mirror(&machine)
        },
        MirrorImport {
            folder: "~/widget".into(),
            ..mirror(&machine)
        },
        MirrorImport {
            url: long,
            ..mirror(&machine)
        },
        MirrorImport {
            url: "https://example.com/\nx".into(),
            ..mirror(&machine)
        },
    ];
    for case in cases {
        bad_request(a.mirror(&case).await);
    }
    assert!(a.list().await.is_empty());
}
