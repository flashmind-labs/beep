//! Hosted login over real HTTP: a guest key asks for a code, the website signs a token, and the
//! key signs in with it.

use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use beep_server::{
    account::login::LoginConfig,
    agent::models::ModelChoices,
    api::http,
    app::{App, ServerTools},
    protocol::{
        ApiError, CompleteLogin, Health, ListDevices, SecretText, StartLogin, client::Client,
    },
    push::Pulse,
    store::TestDb,
};
use flashmind::types::{
    AgentLlmConfig, CompletionRequest, CompletionStream, LlmProvider, Provider,
};
use iroh::SecretKey;
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use url::Url;

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
        unreachable!("the login tests make no model calls")
    }
}

struct Harness {
    _db: TestDb,
    apps: http::Local,
    website: SecretKey,
}

impl Harness {
    async fn new() -> Self {
        let website = SecretKey::generate();
        let config = toml::from_str::<LoginConfig>(&format!(
            "website = \"https://example.com/\"\nwebsite_key = \"{}\"",
            hex(website.public().as_bytes())
        ))
        .expect("a valid login table");
        let (db, apps) = serve(Some(config)).await;
        Self {
            _db: db,
            apps,
            website,
        }
    }

    fn client(&self, identity: SecretKey) -> Client {
        Client::new(self.apps.url(), identity).expect("a client")
    }

    /// A token as the website signs it.
    fn token(&self, code: &str, sub: &str) -> SecretText {
        sign(
            &self.website,
            &json!({
                "v": 1,
                "code": code,
                "sub": sub,
                "email": "a@example.com",
                "name": "A Person",
                "exp": unix_now() + 300,
            }),
        )
    }
}

async fn serve(login: Option<LoginConfig>) -> (TestDb, http::Local) {
    let db = TestDb::new().await.expect("test database");
    let store = db.store().await.expect("test store");
    let app = App::new(
        store,
        Arc::new(NoModel),
        AgentLlmConfig::new("openai:test".parse().expect("a valid model")),
        ServerTools {
            login,
            ..ServerTools::default()
        },
        ModelChoices::only("openai:test".parse().expect("a valid model")),
    );
    let apps = http::Local::start(app, &SecretKey::generate(), Pulse::manual())
        .await
        .expect("the apps' HTTP starts");
    (db, apps)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is past the epoch")
        .as_secs()
}

fn sign(key: &SecretKey, payload: &Value) -> SecretText {
    let payload = URL_SAFE_NO_PAD.encode(payload.to_string());
    let signature = URL_SAFE_NO_PAD.encode(key.sign(payload.as_bytes()).to_bytes());
    SecretText::new(format!("{payload}.{signature}"))
}

/// Asks for a code on `client`, returning what is in the website address.
async fn start(client: &Client) -> String {
    let url = client
        .call(&StartLogin {
            device: "Sam's Mac".into(),
        })
        .await
        .expect("a login address");
    let url = Url::parse(url.expose()).expect("a web address");
    assert_eq!(url.path(), "/login");
    assert_eq!(url.host_str(), Some("example.com"));
    let query = |name: &str| {
        url.query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    };
    assert_eq!(query("device").as_deref(), Some("Sam's Mac"));
    let code = query("code").expect("a code");
    assert_eq!(code.len(), 43);
    assert_eq!(URL_SAFE_NO_PAD.decode(&code).expect("base64url").len(), 32);
    code
}

async fn complete(client: &Client, token: SecretText) -> anyhow::Result<()> {
    client.call(&CompleteLogin { token }).await
}

fn assert_refused(result: anyhow::Result<()>) {
    let err = result.expect_err("the token is refused");
    assert!(
        matches!(
            err.downcast_ref::<ApiError>(),
            Some(ApiError::BadRequest { .. })
        ),
        "{err:#}"
    );
}

fn assert_guest(result: anyhow::Result<impl Sized>) {
    let err = result.err().expect("a guest may not call this");
    assert!(
        matches!(
            err.downcast_ref::<ApiError>(),
            Some(ApiError::Unauthorized {})
        ),
        "{err:#}"
    );
}

#[tokio::test]
async fn a_guest_signs_in_with_the_token_and_is_signed_in_at_once() {
    let h = Harness::new().await;
    let mac = h.client(SecretKey::generate());
    assert!(mac.has_login().await);

    let guest = &mac;
    assert_guest(guest.call(&ListDevices {}).await);
    let code = start(guest).await;
    complete(guest, h.token(&code, "google:1"))
        .await
        .expect("the token signs in");

    let signed_in = &mac;
    signed_in.call(&Health {}).await.expect("it is signed in");
    let devices = signed_in.call(&ListDevices {}).await.expect("devices");
    assert_eq!(devices.len(), 1);
    assert!(devices[0].this_device && devices[0].primary);

    // A second device with the same login joins that account.
    let laptop = h.client(SecretKey::generate());
    let guest = &laptop;
    let code = start(guest).await;
    complete(guest, h.token(&code, "google:1"))
        .await
        .expect("the token signs in");
    let second = &laptop;
    let devices = second.call(&ListDevices {}).await.expect("devices");
    assert_eq!(devices.len(), 2, "both keys are on one account");
    assert!(devices.iter().any(|d| d.this_device && !d.primary));
}

#[tokio::test]
async fn a_code_works_from_another_client_with_the_key() {
    let h = Harness::new().await;
    let key = SecretKey::generate();
    let first = h.client(key.clone());
    let code = start(&first).await;
    drop(first);
    let again = h.client(key);
    complete(&again, h.token(&code, "google:1"))
        .await
        .expect("the code belongs to the key, not the client");
}

#[tokio::test]
async fn a_bad_token_is_refused_and_leaves_the_key_a_guest() {
    let h = Harness::new().await;
    let mac = h.client(SecretKey::generate());
    let stranger = h.client(SecretKey::generate());
    let guest = &mac;
    let other = &stranger;
    let code = start(guest).await;

    // Another key's code: a login link sent to someone else.
    assert_refused(complete(other, h.token(&code, "google:1")).await);
    // Signed by another key.
    let forged = sign(
        &SecretKey::generate(),
        &json!({"v": 1, "code": code, "sub": "google:1", "exp": unix_now() + 300}),
    );
    assert_refused(complete(guest, forged).await);
    // The signature is over the payload's exact characters.
    let token = h.token(&code, "google:1");
    let (payload, signature) = token.expose().split_once('.').expect("two parts");
    let changed = format!("{payload}{}.{signature}", "A");
    assert_refused(complete(guest, SecretText::new(changed)).await);
    // Past its `exp`.
    let late = sign(
        &h.website,
        &json!({"v": 1, "code": code, "sub": "google:1", "exp": unix_now() - 1}),
    );
    assert_refused(complete(guest, late).await);
    // Another version.
    let newer = sign(
        &h.website,
        &json!({"v": 2, "code": code, "sub": "google:1", "exp": unix_now() + 300}),
    );
    assert_refused(complete(guest, newer).await);
    // A code the server never made.
    assert_refused(complete(guest, h.token("nope", "google:1")).await);
    assert_guest(guest.call(&ListDevices {}).await);

    // None of that used the code up.
    complete(guest, h.token(&code, "google:1"))
        .await
        .expect("the right token still works");
    // And it works only once.
    let signed_in = &mac;
    assert_refused(complete(signed_in, h.token(&code, "google:1")).await);
}

#[tokio::test]
async fn a_signed_in_key_attaches_a_login_unless_another_account_holds_it() {
    let h = Harness::new().await;
    let mac = h.client(SecretKey::generate());
    let guest = &mac;
    let code = start(guest).await;
    complete(guest, h.token(&code, "google:1"))
        .await
        .expect("the token signs in");
    let first = &mac;

    // A second login on the same account, then the first one again.
    let code = start(first).await;
    complete(first, h.token(&code, "apple:1"))
        .await
        .expect("a new login attaches");
    let code = start(first).await;
    complete(first, h.token(&code, "google:1"))
        .await
        .expect("its own login is fine");

    let other = h.client(SecretKey::generate());
    let guest = &other;
    let code = start(guest).await;
    complete(guest, h.token(&code, "email:b@example.com"))
        .await
        .expect("the token signs in");
    let second = &other;
    let code = start(second).await;
    let err = complete(second, h.token(&code, "google:1"))
        .await
        .expect_err("another account holds the login");
    assert!(
        matches!(
            err.downcast_ref::<ApiError>(),
            Some(ApiError::Conflict { .. })
        ),
        "{err:#}"
    );
    // Still on its own account, alone.
    let devices = second.call(&ListDevices {}).await.expect("devices");
    assert_eq!(devices.len(), 1);
}

#[tokio::test]
async fn without_a_login_section_nothing_changes() {
    let (_db, apps) = serve(None).await;
    let client = Client::new(apps.url(), SecretKey::generate()).expect("a client");
    assert!(!client.has_login().await, "the check makes no account");
    let devices = client.call(&ListDevices {}).await.expect("devices");
    assert_eq!(devices.len(), 1, "an unknown key makes its own account");
    let err = client
        .call(&StartLogin {
            device: "Mac".into(),
        })
        .await
        .expect_err("no sign-in here");
    assert!(
        matches!(
            err.downcast_ref::<ApiError>(),
            Some(ApiError::BadRequest { .. })
        ),
        "{err:#}"
    );
}
