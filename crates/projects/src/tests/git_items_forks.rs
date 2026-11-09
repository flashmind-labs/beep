use super::*;
use secrecy::SecretString;
use std::{collections::HashMap, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Semaphore,
};

/// A host that answers `routes`, 404s otherwise, and checks the token.
async fn host(routes: HashMap<&'static str, &'static str>) -> String {
    paged_host(routes, HashMap::new()).await
}

/// `host`, whose reply to each path in `next` links to the page there.
async fn paged_host(
    routes: HashMap<&'static str, &'static str>,
    next: HashMap<&'static str, &'static str>,
) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
    let base = format!("http://{}", listener.local_addr().expect("an address"));
    let here = base.clone();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut data = Vec::new();
            let mut buf = [0u8; 4096];

            while !data.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buf).await.expect("the request reads");

                if n == 0 {
                    break;
                }

                data.extend_from_slice(&buf[..n]);
            }

            let request = String::from_utf8_lossy(&data).into_owned();
            let path = request.split(' ').nth(1).unwrap_or_default().to_owned();
            let authorized = request
                .to_ascii_lowercase()
                .contains("authorization: token tok");
            let (status, body) = match routes.get(path.as_str()) {
                Some(body) if authorized => ("200 OK", *body),
                Some(_) => ("401 Unauthorized", "{}"),
                None => ("404 Not Found", r#"{"message":"Not Found"}"#),
            };
            let link = next
                .get(path.as_str())
                .map(|next| format!("Link: <{here}{next}>; rel=\"next\"\r\n"))
                .unwrap_or_default();
            let reply = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{link}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(reply.as_bytes())
                .await
                .expect("the reply writes");
        }
    });
    base
}

async fn look(
    base: &str,
    api: Api,
    upstream: &str,
    login: Option<&str>,
) -> Result<Option<Fork>, HostError> {
    let client = beep_proto::https::client().build().expect("a client");
    let calls = Arc::new(Semaphore::new(4));
    let token = SecretString::from("tok");
    let caller = Caller {
        client: &client,
        calls: &calls,
        host: "github.com",
        token: &token,
    };
    let upstream = RepoName::from_str(upstream).expect("a repository name");
    find(
        &caller,
        &Lookup {
            api,
            base,
            upstream: &upstream,
            login,
        },
    )
    .await
}

const UPSTREAM: &str = r#"{"full_name": "acme/widget", "default_branch": "main", "fork": false,
        "owner": {"login": "acme"}}"#;
const PULLS: &str = r#"[
      {"number": 7, "user": {"login": "octo"},
       "head": {"ref": "fix-retry", "repo": {"full_name": "octo/widget"}}},
      {"number": 8, "user": {"login": "octo"},
       "head": {"ref": "on-upstream", "repo": {"full_name": "acme/widget"}}},
      {"number": 9, "user": {"login": "someone"},
       "head": {"ref": "theirs", "repo": {"full_name": "someone/widget"}}},
      {"number": 10, "user": {"login": "octo"},
       "head": {"ref": "--upload-pack=x", "repo": {"full_name": "octo/widget"}}},
      {"number": 11, "user": {"login": "Octo"},
       "head": {"ref": "dark-theme", "repo": {"full_name": "Octo/Widget"}}},
      {"number": 12, "user": {"login": "octo"}, "head": {"ref": "gone", "repo": null}}
    ]"#;

#[tokio::test]
async fn a_fork_under_the_login_with_its_pull_request_branches() {
    let base = host(HashMap::from([
        ("/user", r#"{"login": "octo"}"#),
        ("/repos/acme/widget", UPSTREAM),
        (
            "/repos/octo/widget",
            r#"{"full_name": "octo/widget", "default_branch": "main", "fork": true,
                    "parent": {"full_name": "acme/widget", "default_branch": "main"}}"#,
        ),
        ("/repos/acme/widget/pulls?state=open&per_page=50", PULLS),
    ]))
    .await;
    let fork = look(&base, Api::GitHub, "acme/widget", None)
        .await
        .expect("the host answers")
        .expect("a fork");
    assert_eq!(fork.name.to_string(), "octo/widget");
    assert_eq!(fork.default_branch, "main");
    assert_eq!(fork.branches, ["fix-retry", "dark-theme"]);
}

#[tokio::test]
async fn the_fork_and_branches_on_later_pages_are_found() {
    let base = paged_host(
        HashMap::from([
            ("/repos/acme/widget", UPSTREAM),
            (
                "/repos/octo/widget",
                r#"{"full_name": "octo/widget", "fork": false}"#,
            ),
            (
                "/repos/acme/widget/forks?per_page=50",
                r#"[{"full_name": "someone/widget", "owner": {"login": "someone"}}]"#,
            ),
            (
                "/repos/acme/widget/forks?per_page=50&page=2",
                r#"[{"full_name": "octo/widget-fork", "owner": {"login": "octo"}}]"#,
            ),
            (
                "/repos/acme/widget/pulls?state=open&per_page=50",
                r#"[{"user": {"login": "octo"},
                         "head": {"ref": "first", "repo": {"full_name": "octo/widget-fork"}}}]"#,
            ),
            (
                "/repos/acme/widget/pulls?state=open&per_page=50&page=2",
                r#"[{"user": {"login": "octo"},
                         "head": {"ref": "second", "repo": {"full_name": "octo/widget-fork"}}}]"#,
            ),
        ]),
        HashMap::from([
            (
                "/repos/acme/widget/forks?per_page=50",
                "/repos/acme/widget/forks?per_page=50&page=2",
            ),
            (
                "/repos/acme/widget/pulls?state=open&per_page=50",
                "/repos/acme/widget/pulls?state=open&per_page=50&page=2",
            ),
        ]),
    )
    .await;
    let fork = look(&base, Api::GitHub, "acme/widget", Some("octo"))
        .await
        .expect("the host answers")
        .expect("a fork");
    assert_eq!(fork.name.to_string(), "octo/widget-fork");
    assert_eq!(fork.branches, ["first", "second"]);
}
