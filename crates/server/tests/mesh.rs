//! A desktop that imports a project on its own tells the server, so phones
//! and the project agent still see it.

use async_trait::async_trait;
use beep_server::{
    agent::models::ModelChoices,
    api::transport,
    app::{App, ServerTools},
    client::{Client, Node},
    protocol::{ApiError, DeviceKind, ListImportedProjects, MirrorImport},
    push::Pusher,
    store::{NewKey, TestDb},
};
use flashmind::types::{
    AgentLlmConfig, CompletionRequest, CompletionStream, LlmProvider, Provider,
};
use iroh::{Endpoint, SecretKey, endpoint::presets, protocol::Router};
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
    _router: Router,
    _phone_node: Node,
    phone: Client,
    /// The key of the caller's linked machine.
    machine: String,
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
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .bind()
        .await
        .expect("the server binds");
    let pusher = Pusher::new(endpoint.clone(), Arc::new(app.store.clone()), None);
    let router = Router::builder(endpoint)
        .accept(
            transport::ALPN,
            transport::BeepHandler::new(app.clone(), pusher),
        )
        .spawn();
    let identity = SecretKey::generate();
    let user = app
        .sign_in(&identity.public())
        .await
        .expect("sign in")
        .expect("a new key is never revoked")
        .user;
    let machine = SecretKey::generate().public();
    user.link_key(NewKey {
        endpoint: machine,
        name: "build box",
        kind: DeviceKind::Desktop {},
    })
    .await
    .expect("the machine links");
    let phone_node = Node::bind(identity, router.endpoint().addr(), Arc::new(|_| true))
        .await
        .expect("a phone binds");
    let phone = phone_node.connect().await.expect("a phone connects");
    Harness {
        _db: db,
        _router: router,
        _phone_node: phone_node,
        phone,
        machine: machine.to_string(),
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

#[tokio::test(flavor = "multi_thread")]
async fn a_mirrored_import_shows_in_the_servers_project_list() {
    let h = harness().await;
    h.phone.call(&mirror(&h.machine)).await.unwrap();
    h.phone.call(&mirror(&h.machine)).await.unwrap();

    let listed = h.phone.call(&ListImportedProjects {}).await.unwrap();
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0].project, "org/widget");
    assert_eq!(listed[0].machine_id, h.machine);
    assert_eq!(listed[0].folder, "/srv/widget");
    assert_eq!(listed[0].url, "https://example.com/org/widget");
    assert!(!listed[0].cloned);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_mirrored_import_on_a_machine_that_is_not_the_callers_is_refused() {
    let h = harness().await;
    let stranger = SecretKey::generate().public().to_string();
    let refused = h
        .phone
        .call(&mirror(&stranger))
        .await
        .unwrap_err()
        .downcast::<ApiError>()
        .expect("the server answers with an ApiError");
    assert_eq!(refused, ApiError::NotFound {});
    assert!(
        h.phone
            .call(&ListImportedProjects {})
            .await
            .unwrap()
            .is_empty()
    );
}
