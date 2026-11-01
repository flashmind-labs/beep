//! Single-user instance mode: who may sign in, and what the owner gets.

use async_trait::async_trait;
use beep_server::{
    app::{Admission, App, ServerTools, SignedIn},
    events,
    instance::{InstanceConfig, NODE_FILE, Privilege},
    models::ModelChoices,
    protocol::DeviceKind,
    store::{NewKey, Store, TestDb},
};
use flashmind::types::{
    AgentLlmConfig, CompletionRequest, CompletionStream, LlmProvider, Provider,
};
use iroh::{EndpointId, SecretKey};
use std::{fs, path::PathBuf, sync::Arc};

/// The instance tests never run a model turn.
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
        unreachable!("the instance tests make no model calls")
    }
}

const PRIVILEGES: [Privilege; 2] = [Privilege::Events, Privilege::Nodes];

fn instance(text: &str) -> InstanceConfig {
    toml::from_str(text).expect("a valid instance table")
}

async fn app(store: Store, admins: &[EndpointId], instance: Option<InstanceConfig>) -> Arc<App> {
    let model = "openai:test".parse().expect("a valid model");
    let models = ModelChoices::resolve(&NoModel, model, Vec::new(), admins).await;
    App::new(
        store,
        Arc::new(NoModel),
        AgentLlmConfig::new("openai:test".parse().expect("a valid model")),
        ServerTools {
            instance,
            ..ServerTools::default()
        },
        models,
    )
}

async fn signed_in(app: &Arc<App>, key: &SecretKey) -> SignedIn {
    match app.admit(&key.public()).await.expect("the store answers") {
        Admission::SignedIn(signed_in) => signed_in,
        Admission::Revoked => panic!("the key was revoked"),
        Admission::NotInvited | Admission::Guest => panic!("the key was refused"),
    }
}

/// The key does not sign in: refused, or a guest that may only join.
async fn refused(app: &Arc<App>, key: &SecretKey) -> bool {
    matches!(
        app.admit(&key.public()).await.expect("the store answers"),
        Admission::NotInvited | Admission::Guest
    )
}

#[tokio::test]
async fn a_new_phone_joins_the_owner_with_a_code() {
    let db = TestDb::new().await.expect("test database");
    let store = db.store().await.expect("store");
    let app = app(store.clone(), &[], Some(instance(""))).await;
    let (phone, second) = (SecretKey::generate(), SecretKey::generate());
    let owner = signed_in(&app, &phone).await;
    assert!(matches!(
        app.admit(&second.public())
            .await
            .expect("the store answers"),
        Admission::Guest
    ));

    let invite = owner
        .user
        .create_invite(
            &phone.public(),
            "phone",
            std::time::Duration::from_secs(600),
        )
        .await
        .expect("a code");
    let key = || NewKey {
        endpoint: second.public(),
        name: "iPhone",
        kind: DeviceKind::Phone {},
    };
    let joined = store
        .redeem_invite(&invite.code, key())
        .await
        .expect("the store answers");
    assert_eq!(joined.as_deref(), Some(owner.user.id()));
    let linked = signed_in(&app, &second).await;
    assert_eq!(linked.user.id(), owner.user.id());
    assert!(!linked.primary);
    assert!(
        store
            .redeem_invite(&invite.code, key())
            .await
            .expect("the store answers")
            .is_none(),
        "the code is used up"
    );
}

#[tokio::test]
async fn the_first_key_owns_the_instance_and_others_are_refused() {
    let db = TestDb::new().await.expect("test database");
    let app = app(db.store().await.expect("store"), &[], Some(instance(""))).await;
    let (phone, stranger, laptop) = (
        SecretKey::generate(),
        SecretKey::generate(),
        SecretKey::generate(),
    );

    let owner = signed_in(&app, &phone).await;
    assert!(owner.primary);
    assert!(refused(&app, &stranger).await, "a second key is refused");
    assert!(refused(&app, &stranger).await, "and stays refused");

    // A device the owner links signs into the owner's account.
    owner
        .user
        .link_key(NewKey {
            endpoint: laptop.public(),
            name: "laptop",
            kind: DeviceKind::Desktop {},
        })
        .await
        .expect("the key is linked");
    let linked = signed_in(&app, &laptop).await;
    assert_eq!(linked.user.id(), owner.user.id());
    assert!(!linked.primary);

    // After a restart the owner is still the owner, and an admin before
    // connecting again.
    let again = self::app(db.store().await.expect("store"), &[], Some(instance(""))).await;
    again.start_users().await.expect("users start");
    let user = again.user(owner.user.id()).expect("the owner started");
    for privilege in PRIVILEGES {
        assert!(again.allows(privilege, &user), "{privilege:?}");
    }
    assert!(refused(&again, &stranger).await);
    assert_eq!(signed_in(&again, &phone).await.user.id(), owner.user.id());
}

#[tokio::test]
async fn only_the_configured_owner_may_claim() {
    let db = TestDb::new().await.expect("test database");
    let phone = SecretKey::generate();
    let config = instance(&format!("owner = \"{}\"", phone.public()));
    let app = app(db.store().await.expect("store"), &[], Some(config)).await;
    let first = SecretKey::generate();
    assert!(
        refused(&app, &first).await,
        "the first key is not the owner"
    );
    let owner = signed_in(&app, &phone).await;
    assert!(owner.primary);
    assert!(refused(&app, &first).await);
}

#[tokio::test]
async fn an_account_from_before_claims_the_instance() {
    let db = TestDb::new().await.expect("test database");
    let (phone, other) = (SecretKey::generate(), SecretKey::generate());
    let shared = app(db.store().await.expect("store"), &[], None).await;
    let before = signed_in(&shared, &phone).await;
    let other_before = signed_in(&shared, &other).await;
    assert_ne!(before.user.id(), other_before.user.id());

    let app = app(db.store().await.expect("store"), &[], Some(instance(""))).await;
    assert_eq!(signed_in(&app, &phone).await.user.id(), before.user.id());
    assert!(
        refused(&app, &other).await,
        "the other account is not the owner's"
    );
}

#[tokio::test]
async fn the_owner_gets_what_the_settings_leave_on() {
    let db = TestDb::new().await.expect("test database");
    let config = instance("events = false\nnodes = false");
    let app = app(db.store().await.expect("store"), &[], Some(config)).await;
    let owner = signed_in(&app, &SecretKey::generate()).await.user;
    assert!(!app.allows(Privilege::Events, &owner));
    assert!(!events::enabled(&app, &owner));
    assert!(!app.allows(Privilege::Nodes, &owner));
}

#[tokio::test]
async fn a_shared_server_keeps_its_admins_and_open_sign_up() {
    let db = TestDb::new().await.expect("test database");
    let admin = SecretKey::generate();
    let app = app(db.store().await.expect("store"), &[admin.public()], None).await;
    let admin_user = signed_in(&app, &admin).await.user;
    let someone = signed_in(&app, &SecretKey::generate()).await.user;
    assert_ne!(admin_user.id(), someone.id());
    for privilege in PRIVILEGES {
        assert!(app.allows(privilege, &admin_user), "{privilege:?}");
        assert!(!app.allows(privilege, &someone), "{privilege:?}");
    }
    assert!(events::enabled(&app, &admin_user));
    assert!(!events::enabled(&app, &someone));
}

#[tokio::test]
async fn the_bundled_node_joins_the_owner() {
    let db = TestDb::new().await.expect("test database");
    let dir = tempfile::tempdir().expect("a temp dir");
    let link: PathBuf = dir.path().to_owned();
    let config = InstanceConfig {
        local_node: Some(link.clone()),
        ..instance("")
    };
    let app = app(db.store().await.expect("store"), &[], Some(config)).await;
    let node = SecretKey::generate();
    fs::write(
        link.join(NODE_FILE),
        format!("{}\nhome box\n", node.public()),
    )
    .expect("the node file is written");

    // Nobody owns the instance yet, so the node waits.
    let waiting = app.sign_in_linked(&node.public()).await.expect("answers");
    assert!(waiting.is_none());

    let owner = signed_in(&app, &SecretKey::generate()).await.user;
    let joined = app
        .sign_in_linked(&node.public())
        .await
        .expect("answers")
        .expect("the node signs in");
    assert_eq!(joined.user.id(), owner.id());
    let keys = owner.account_keys().await.expect("the keys");
    assert!(
        keys.iter()
            .any(|key| key.endpoint_id == node.public().to_string() && key.name == "home box")
    );

    // Another key that no one linked is no node of the owner's.
    let stranger = SecretKey::generate();
    assert!(
        app.sign_in_linked(&stranger.public())
            .await
            .expect("answers")
            .is_none()
    );
}
