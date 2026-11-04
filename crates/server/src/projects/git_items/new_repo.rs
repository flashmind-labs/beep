//! Making a repository on GitHub, Forgejo or Gitea with the user's token, and who it can belong to.

use super::{
    forks::{Account, Api, all_pages},
    host::{Caller, HostError, status_error},
};
use crate::projects::model::RepoName;
use futures::future::join_all;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

/// A host's API, and the token's account when it is known already.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Forge<'a> {
    pub(crate) api: Api,
    pub(crate) base: &'a str,
    pub(crate) login: Option<&'a str>,
}

impl Forge<'_> {
    async fn login(&self, caller: &Caller<'_>) -> Result<String, HostError> {
        match self.login {
            Some(login) => Ok(login.to_owned()),
            None => {
                let me: Account = caller.get(&format!("{}/user", self.base)).await?;
                Ok(me.login)
            }
        }
    }
}

/// An organization: GitHub calls it `login`, Forgejo `name`.
#[derive(Debug, Deserialize)]
struct Org {
    #[serde(default)]
    login: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OrgPermissions {
    can_create_repository: bool,
}

/// The token's account, then the organizations it can create in.
pub(crate) async fn owners(
    caller: &Caller<'_>,
    forge: &Forge<'_>,
) -> Result<Vec<String>, HostError> {
    let login = forge.login(caller).await?;
    let first = format!("{}/user/orgs?{}", forge.base, forge.api.page());
    let orgs: Vec<Org> = match all_pages(caller, first, |_| false).await {
        Ok(orgs) => orgs,
        // A token that cannot read organizations can still create under its own account.
        Err(e) => {
            log::info!("listing organizations: {e}");
            Vec::new()
        }
    };
    let names = orgs.into_iter().filter_map(|org| org.login.or(org.name));
    let kept = join_all(names.map(|org| {
        let login = &login;
        async move { can_create(caller, forge, login, &org).await.then_some(org) }
    }))
    .await;
    let mut all = vec![login];
    all.extend(kept.into_iter().flatten());
    Ok(all)
}

/// Whether `login` can create repositories in `org`. GitHub only tells an org's owners, so there the
/// answer waits for the create call.
async fn can_create(caller: &Caller<'_>, forge: &Forge<'_>, login: &str, org: &str) -> bool {
    if forge.api == Api::GitHub {
        return true;
    }
    let url = format!("{}/users/{login}/orgs/{org}/permissions", forge.base);
    match caller.get::<OrgPermissions>(&url).await {
        Ok(permissions) => permissions.can_create_repository,
        Err(e) => {
            log::info!("reading permissions in {org}: {e}");
            false
        }
    }
}

/// The repository to make.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NewRepo<'a> {
    pub(crate) owner: &'a str,
    pub(crate) name: &'a str,
    pub(crate) private: bool,
}

#[derive(Debug, Serialize)]
struct Body<'a> {
    name: &'a str,
    private: bool,
    /// A first commit, so the clones have a branch to work from.
    auto_init: bool,
}

#[derive(Debug, Deserialize)]
struct Made {
    full_name: String,
}

/// Why the host made no repository.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CreateError {
    NameTaken,
    /// The token lacks the scope, or the account may not create in that owner.
    Denied,
    Host(HostError),
}

/// Creates the repository and gives its name as the host wrote it.
pub(crate) async fn create(
    caller: &Caller<'_>,
    forge: &Forge<'_>,
    new: &NewRepo<'_>,
) -> Result<RepoName, CreateError> {
    let login = forge.login(caller).await.map_err(CreateError::Host)?;
    let url = if new.owner.eq_ignore_ascii_case(&login) {
        format!("{}/user/repos", forge.base)
    } else {
        format!("{}/orgs/{}/repos", forge.base, new.owner)
    };
    let body = Body {
        name: new.name,
        private: new.private,
        auto_init: true,
    };
    let reply = caller.post(&url, &body).await.map_err(CreateError::Host)?;
    let host = caller.host.to_owned();
    let status = reply.status();
    if status.is_success() {
        let bad = || CreateError::Host(HostError::BadReply(host.clone()));
        let made: Made = reply.json().await.map_err(|_| bad())?;
        return RepoName::parse(&made.full_name).map_err(|_| bad());
    }
    let headers = reply.headers().clone();
    // Read only for GitHub's sentence; a reply that cannot be read counts as any other refusal.
    let said = reply.text().await.unwrap_or_default();
    Err(match status_error(host, status, &headers) {
        limited @ HostError::RateLimited(_) => CreateError::Host(limited),
        _ if status == StatusCode::CONFLICT => CreateError::NameTaken,
        _ if status == StatusCode::UNPROCESSABLE_ENTITY && said.contains("already exists") => {
            CreateError::NameTaken
        }
        _ if status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND => {
            CreateError::Denied
        }
        other => CreateError::Host(other),
    })
}

#[cfg(test)]
mod tests {
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
}
