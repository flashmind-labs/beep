//! `POST /v1/reports` against a server on a local port with a folder store.

use std::fs::{read, read_to_string};
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use beep_proto::ApiError;
use beep_proto::report::{
    KEY_HEADER, MAX_BODY, PATH, Receipt, SIGNATURE_HEADER, TIME_HEADER, report_id, signed_message,
};
use beep_report::{Manifest, Period, SendError, Sender, draft, send, zip};
use beep_server::config::ReportsConfig;
use beep_server::reports::{Reports, routes};
use iroh::{EndpointId, SecretKey};
use reqwest::{Client, StatusCode};
use tempfile::TempDir;
use tokio::net::TcpListener;
use url::Url;

struct Server {
    url: Url,
    dir: TempDir,
}

impl Server {
    /// Nothing at all was written to the store.
    fn is_empty(&self) -> bool {
        !self.dir.path().join("reports").exists()
    }
}

async fn serve(config: ReportsConfig) -> Server {
    let dir = tempfile::tempdir().expect("a temp dir");
    let config = ReportsConfig {
        folder: config.folder.or_else(|| Some(dir.path().to_owned())),
        ..config
    };
    Server {
        url: serve_with(&config).await,
        dir,
    }
}

async fn serve_with(config: &ReportsConfig) -> Url {
    let reports = Reports::new(config, Client::new()).expect("the reports config is valid");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a free local port binds");
    let addr = listener
        .local_addr()
        .expect("a bound listener has an address");
    let app = routes(reports).into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move { axum::serve(listener, app).await });
    Url::parse(&format!("http://{addr}")).expect("a local address is a URL")
}

fn now_ms() -> u64 {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after 1970");
    u64::try_from(since.as_millis()).expect("milliseconds fit in u64")
}

/// A report zip from `sender`, in `mesh`; `text` makes each one different.
fn report(sender: EndpointId, mesh: Option<EndpointId>, text: &str) -> Vec<u8> {
    let sender = Sender {
        id: sender,
        mesh,
        app: "desktop",
        app_version: "0.2.0".into(),
    };
    let draft = draft(text, Period::Today, 0, sender, Vec::new()).expect("a draft builds");
    zip(&draft).expect("the draft zips")
}

/// What a post signed by `signer`, under `key`'s name, at `at_ms` gets back.
struct Post<'a> {
    key: &'a SecretKey,
    signer: &'a SecretKey,
    at_ms: u64,
    ip: Option<&'a str>,
}

impl<'a> Post<'a> {
    fn by(key: &'a SecretKey) -> Self {
        Self {
            key,
            signer: key,
            at_ms: now_ms(),
            ip: None,
        }
    }

    async fn send(self, server: &Url, body: Vec<u8>) -> (StatusCode, String) {
        let signature = self.signer.sign(&signed_message(&body, self.at_ms));
        let mut request = Client::new()
            .post(format!("{}{PATH}", server.as_str().trim_end_matches('/')))
            .header(KEY_HEADER, self.key.public().to_string())
            .header(TIME_HEADER, self.at_ms.to_string())
            .header(SIGNATURE_HEADER, hex::encode(signature.to_bytes()));
        if let Some(ip) = self.ip {
            request = request.header("CF-Connecting-IP", ip);
        }
        let reply = request.body(body).send().await.expect("the server answers");
        let status = reply.status();
        let bytes = reply.bytes().await.expect("the reply reads");
        let text = match rmp_serde::from_slice::<ApiError>(&bytes) {
            Ok(ApiError::BadRequest { message }) => message,
            other => format!("{other:?}"),
        };
        (status, text)
    }
}

fn refused(error: SendError) -> String {
    match error {
        SendError::Refused(text) => text,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn a_signed_report_is_stored_and_gets_a_reference() {
    let server = serve(ReportsConfig::default()).await;
    let key = SecretKey::generate();
    let mesh = SecretKey::generate().public();
    let body = report(key.public(), Some(mesh), "the bar froze");
    let receipt: Receipt = send(&server.url, body.clone(), &key)
        .await
        .expect("the report is taken");
    let id = report_id(&body);
    assert_eq!(receipt.reference, format!("R-{}", id.to_uppercase()));

    let folder = server.dir.path().join(format!("reports/{mesh}"));
    assert_eq!(
        read(folder.join(format!("{id}.zip"))).expect("the zip"),
        body
    );
    let json = read_to_string(folder.join(format!("{id}.json"))).expect("the manifest");
    let manifest: Manifest = serde_json::from_str(&json).expect("the manifest reads");
    assert_eq!(manifest.sender, key.public());
    assert_eq!(manifest.text, "the bar froze");
}

#[tokio::test]
async fn a_report_in_no_mesh_is_kept_under_its_sender() {
    let server = serve(ReportsConfig::default()).await;
    let key = SecretKey::generate();
    let body = report(key.public(), None, "no mesh yet");
    send(&server.url, body.clone(), &key)
        .await
        .expect("the report is taken");
    let zip = Path::new("reports")
        .join(key.public().to_string())
        .join(format!("{}.zip", report_id(&body)));
    assert_eq!(read(server.dir.path().join(zip)).expect("the zip"), body);
}

#[tokio::test]
async fn a_report_signed_six_minutes_ago_is_refused_with_the_clock_words() {
    let server = serve(ReportsConfig::default()).await;
    let key = SecretKey::generate();
    let body = report(key.public(), None, "late");
    let post = Post {
        at_ms: now_ms() - 6 * 60_000,
        ..Post::by(&key)
    };
    let (status, text) = post.send(&server.url, body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(text.contains("clock is off"), "{text}");
    assert_eq!(
        text,
        "This computer's clock is off by 6 minutes; set it right and send again."
    );
    assert!(server.is_empty());
}

#[tokio::test]
async fn a_report_with_a_wrong_signature_is_refused() {
    let server = serve(ReportsConfig::default()).await;
    let key = SecretKey::generate();
    let other = SecretKey::generate();
    let body = report(key.public(), None, "forged");
    let post = Post {
        signer: &other,
        ..Post::by(&key)
    };
    let (status, _) = post.send(&server.url, body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(server.is_empty());
}

#[tokio::test]
async fn a_body_over_five_megabytes_is_refused() {
    let server = serve(ReportsConfig::default()).await;
    let key = SecretKey::generate();
    let (status, _) = Post::by(&key)
        .send(&server.url, vec![0; MAX_BODY + 1])
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(server.is_empty());
}

#[tokio::test]
async fn a_manifest_naming_another_sender_is_refused() {
    let server = serve(ReportsConfig::default()).await;
    let key = SecretKey::generate();
    let body = report(SecretKey::generate().public(), None, "someone else");
    let got = send(&server.url, body, &key).await.expect_err("refused");
    assert_eq!(refused(got), "The report names another sender.");
    assert!(server.is_empty());
}

#[tokio::test]
async fn a_blocked_key_is_refused() {
    let key = SecretKey::generate();
    let server = serve(ReportsConfig {
        blocked_keys: vec![key.public()],
        ..ReportsConfig::default()
    })
    .await;
    let body = report(key.public(), None, "blocked");
    let (status, text) = Post::by(&key).send(&server.url, body).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(text, "refused");
    assert!(server.is_empty());
}

#[tokio::test]
async fn a_blocked_address_is_refused_from_cloudflare_or_the_peer() {
    let cloudflare: IpAddr = "192.0.2.7".parse().expect("an address");
    let server = serve(ReportsConfig {
        blocked_ips: vec![cloudflare, IpAddr::from([127, 0, 0, 1])],
        ..ReportsConfig::default()
    })
    .await;
    let key = SecretKey::generate();
    let body = report(key.public(), None, "from a blocked place");
    for ip in [Some("192.0.2.7"), None] {
        let post = Post {
            ip,
            ..Post::by(&key)
        };
        let (status, text) = post.send(&server.url, body.clone()).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{ip:?}");
        assert_eq!(text, "refused");
    }
    let post = Post {
        ip: Some("198.51.100.1"),
        ..Post::by(&key)
    };
    let (status, _) = post.send(&server.url, body).await;
    assert_eq!(status, StatusCode::OK, "Cloudflare's address wins");
}

#[tokio::test]
async fn the_twenty_first_report_of_a_day_is_refused() {
    let server = serve(ReportsConfig {
        per_key_per_day: 2,
        ..ReportsConfig::default()
    })
    .await;
    let key = SecretKey::generate();
    let forger = SecretKey::generate();
    let forged = Post {
        signer: &forger,
        ..Post::by(&key)
    };
    let (status, _) = forged
        .send(&server.url, report(key.public(), None, "forged"))
        .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a refused one uses nothing up"
    );
    for n in 0..2 {
        let body = report(key.public(), None, &format!("report {n}"));
        send(&server.url, body, &key)
            .await
            .expect("within the limit");
    }
    let body = report(key.public(), None, "one too many");
    let (status, _) = Post::by(&key).send(&server.url, body).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    let other = SecretKey::generate();
    let body = report(other.public(), None, "another computer");
    send(&server.url, body, &other)
        .await
        .expect("another key has its own count");
}

#[tokio::test]
async fn a_report_the_store_fails_to_keep_does_not_use_up_the_day() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let folder = dir.path().join("store");
    // A file where the store's folder should be, so every write fails.
    std::fs::write(&folder, b"in the way").expect("the file writes");
    let url = serve_with(&ReportsConfig {
        folder: Some(folder.clone()),
        per_key_per_day: 1,
        ..ReportsConfig::default()
    })
    .await;
    let key = SecretKey::generate();
    let (status, _) = Post::by(&key)
        .send(&url, report(key.public(), None, "first try"))
        .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    std::fs::remove_file(&folder).expect("the file goes");
    send(&url, report(key.public(), None, "second try"), &key)
        .await
        .expect("the failed one was given back");
}

#[test]
fn reports_outside_a_runtime_is_an_error() {
    assert!(Reports::new(&ReportsConfig::default(), Client::new()).is_err());
}

#[tokio::test]
async fn without_a_store_the_route_says_reports_are_not_set_up() {
    let url = serve_with(&ReportsConfig::default()).await;
    let key = SecretKey::generate();
    let body = report(key.public(), None, "nowhere to go");
    let (status, text) = Post::by(&key).send(&url, body).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(text, "Problem reports are not set up on this server yet.");
}
