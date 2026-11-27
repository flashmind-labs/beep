use super::*;
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
    let client = beep_proto::https::client().build().expect("a client");
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

#[tokio::test]
async fn repository_rename_uses_the_host_api_and_reports_permission_failures() {
    use crate::{
        credentials::{GitHubLogin, Opened},
        imports::{
            hosts::{Hosts, rename_repository},
            repo::RepoRef,
        },
    };
    use beep_proto::Service;
    struct HostsForTest {
        client: reqwest::Client,
        calls: Arc<Semaphore>,
    }
    impl Hosts for HostsForTest {
        fn client(&self) -> anyhow::Result<reqwest::Client> {
            Ok(self.client.clone())
        }
        fn calls(&self) -> &Arc<Semaphore> {
            &self.calls
        }
    }
    for github in [true, false] {
        for status in ["200 OK", "403 Forbidden"] {
            let path = if github {
                "PATCH /repos/team/old"
            } else {
                "PATCH /api/v1/repos/team/old"
            };
            let (base, seen) = forge([(path, (status, "{}"))].into()).await;
            let server = base.parse().unwrap();
            let hosts = HostsForTest {
                client: beep_proto::https::client().build().unwrap(),
                calls: Arc::new(Semaphore::new(2)),
            };
            let opened = Opened {
                service: if github {
                    Service::GitHub
                } else {
                    Service::Forgejo { url: server }
                },
                secret: "tok".into(),
                github: github.then(|| GitHubLogin {
                    login: "owner".into(),
                    orgs: vec!["team".into()],
                    default: true,
                }),
            };
            let servers = crate::imports::hosts::forgejo_of(std::slice::from_ref(&opened));
            let url = if github {
                "https://github.com/team/old".into()
            } else {
                format!("{base}/team/old")
            };
            let repo = RepoRef::parse(&url, &servers).unwrap();
            let result = rename_repository(
                &hosts,
                &[opened],
                &repo,
                &"team/new".parse().unwrap(),
                Some(&base),
            )
            .await;
            if status.starts_with("200") {
                assert_eq!(result.unwrap().project(), "team/new");
            } else {
                assert!(
                    result
                        .unwrap_err()
                        .to_string()
                        .contains("administration access")
                );
            }
            let calls = seen.lock().unwrap();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].line, path);
            assert_eq!(
                serde_json::from_str::<Value>(&calls[0].body).unwrap(),
                json!({"name":"new"})
            );
        }
    }
}
