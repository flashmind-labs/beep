use super::*;
use reqwest::Client;
use secrecy::SecretString;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Semaphore,
};

/// A call the fake forge saw.
#[derive(Debug, Clone)]
struct Call {
    line: String,
    body: String,
}

type Seen = Arc<Mutex<Vec<Call>>>;
type Routes = HashMap<&'static str, (&'static str, &'static str)>;

/// A forge answering `routes`, by `METHOD path`, with a status line and body; 404 otherwise.
async fn forge(routes: Routes) -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
    let base = format!("http://{}", listener.local_addr().expect("an address"));
    let seen = Seen::default();
    let saw = seen.clone();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut data = Vec::new();
            let mut buf = [0u8; 4096];
            let head = loop {
                if let Some(at) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                    break at + 4;
                }
                let n = stream.read(&mut buf).await.expect("the request reads");
                assert_ne!(n, 0, "the request ended early");
                data.extend_from_slice(&buf[..n]);
            };
            let request = String::from_utf8_lossy(&data[..head]).to_ascii_lowercase();
            assert!(request.contains("authorization: token tok"), "{request}");
            let length: usize = request
                .lines()
                .find_map(|line| line.strip_prefix("content-length: ")?.parse().ok())
                .unwrap_or(0);
            while data.len() < head + length {
                let n = stream.read(&mut buf).await.expect("the body reads");
                assert_ne!(n, 0, "the body ended early");
                data.extend_from_slice(&buf[..n]);
            }
            let text = String::from_utf8_lossy(&data[..head]).into_owned();
            let mut words = text.split(' ');
            let line = format!(
                "{} {}",
                words.next().unwrap_or_default(),
                words.next().unwrap_or_default()
            );
            let (status, reply) = routes
                .get(line.as_str())
                .copied()
                .unwrap_or(("404 Not Found", r#"{"message":"Not Found"}"#));
            saw.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(Call {
                    line,
                    body: String::from_utf8_lossy(&data[head..]).into_owned(),
                });
            let reply = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            );
            stream
                .write_all(reply.as_bytes())
                .await
                .expect("the reply writes");
        }
    });
    (base, seen)
}

async fn make(forge: &Forge<'_>, owner: &str) -> Result<RepoName, CreateError> {
    let client = Client::new();
    let calls = Arc::new(Semaphore::new(4));
    let token = SecretString::from("tok");
    let caller = Caller {
        client: &client,
        calls: &calls,
        host: "forge",
        token: &token,
    };
    let new = NewRepo {
        owner,
        name: "widget",
        private: true,
    };
    create(&caller, forge, &new).await
}

const MADE: &str = r#"{"id": 1, "full_name": "Acme/widget", "default_branch": "main",
        "html_url": "https://forge.example.com/Acme/widget"}"#;

#[tokio::test]
async fn a_repository_is_created_under_the_account_or_an_organization() {
    let (base, seen) = forge(HashMap::from([
        ("GET /user", ("200 OK", r#"{"login": "octo"}"#)),
        ("POST /user/repos", ("201 Created", MADE)),
        ("POST /orgs/acme/repos", ("201 Created", MADE)),
    ]))
    .await;
    let forgejo = Forge {
        api: Api::Forgejo,
        base: &base,
        login: None,
    };
    let name = make(&forgejo, "OCTO").await.expect("it is created");
    assert_eq!(name.to_string(), "Acme/widget", "the host's spelling");
    make(&forgejo, "acme").await.expect("it is created");
    let calls = seen.lock().unwrap_or_else(PoisonError::into_inner).clone();
    assert_eq!(calls[1].line, "POST /user/repos");
    assert_eq!(calls[3].line, "POST /orgs/acme/repos");
    let body: Value = serde_json::from_str(&calls[1].body).expect("the body is JSON");
    assert_eq!(
        body,
        json!({"name": "widget", "private": true, "auto_init": true})
    );
}

#[tokio::test]
async fn a_taken_name_or_a_missing_scope_says_so() {
    let github_taken = r#"{"message": "Repository creation failed.", "errors":
            [{"resource": "Repository", "code": "custom", "field": "name",
              "message": "name already exists on this account"}]}"#;
    let (base, _) = forge(HashMap::from([
        (
            "POST /user/repos",
            ("422 Unprocessable Entity", github_taken),
        ),
        ("POST /orgs/forgejo/repos", ("409 Conflict", "{}")),
        (
            "POST /orgs/scopeless/repos",
            ("403 Forbidden", r#"{"message": "token lacks the scope"}"#),
        ),
        (
            "POST /orgs/invalid/repos",
            (
                "422 Unprocessable Entity",
                r#"{"message": "name is reserved"}"#,
            ),
        ),
    ]))
    .await;
    let github = Forge {
        api: Api::GitHub,
        base: &base,
        login: Some("octo"),
    };
    assert_eq!(make(&github, "octo").await, Err(CreateError::NameTaken));
    assert_eq!(make(&github, "forgejo").await, Err(CreateError::NameTaken));
    assert_eq!(make(&github, "scopeless").await, Err(CreateError::Denied));
    assert_eq!(make(&github, "missing").await, Err(CreateError::Denied));
    assert_eq!(
        make(&github, "invalid").await,
        Err(CreateError::Host(HostError::Status {
            host: "forge".to_owned(),
            status: 422
        }))
    );
}

#[tokio::test]
async fn owners_are_the_account_and_the_organizations_it_can_create_in() {
    let (base, _) = forge(HashMap::from([
        ("GET /user", ("200 OK", r#"{"login": "octo"}"#)),
        (
            "GET /user/orgs?limit=50",
            ("200 OK", r#"[{"name": "open"}, {"name": "closed"}]"#),
        ),
        (
            "GET /users/octo/orgs/open/permissions",
            ("200 OK", r#"{"can_create_repository": true}"#),
        ),
        (
            "GET /users/octo/orgs/closed/permissions",
            ("200 OK", r#"{"can_create_repository": false}"#),
        ),
    ]))
    .await;
    let client = Client::new();
    let calls = Arc::new(Semaphore::new(4));
    let token = SecretString::from("tok");
    let caller = Caller {
        client: &client,
        calls: &calls,
        host: "forge",
        token: &token,
    };
    let forgejo = Forge {
        api: Api::Forgejo,
        base: &base,
        login: None,
    };
    assert_eq!(
        owners(&caller, &forgejo).await.expect("the host answers"),
        ["octo", "open"]
    );
}
