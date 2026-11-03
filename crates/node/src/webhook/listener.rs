//! The HTTP listener webhook deliveries come to: one on the loopback for
//! `gh webhook forward`, and one on the address the `webhook_listen`
//! setting names for Forgejo and Gitea servers on the private network. Each
//! watch has its own route, `/beep/webhook/<token>`, and its own secret; a
//! delivery is passed on only when its signature checks out.
//!
//! It reads just enough HTTP/1.1 for a webhook: one POST with a
//! `Content-Length` per connection, then it closes.

use super::signature;
use anyhow::{Context, Result};
use beep_proto::node::webhook::MAX_EVENT;
use secrecy::SecretString;
use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    task::JoinSet,
    time::timeout,
};
use tokio_util::task::AbortOnDropHandle;
use uuid::Uuid;

/// Where each route's path starts.
const PREFIX: &str = "/beep/webhook/";
/// The longest request head read.
const MAX_HEAD: usize = 16 * 1024;
/// The largest delivery read. GitHub sends up to 25 MB, but a delivery
/// that large only says that something changed, which a smaller one says
/// too: a bigger one is refused, and the next poll catches up.
const MAX_BODY: usize = 4 * 1024 * 1024;
/// How long one delivery may take to arrive.
const READ_LIMIT: Duration = Duration::from_secs(10);
/// Connections served at once per listener.
const MAX_CONNECTIONS: usize = 16;
/// Events queued per watch before more are dropped; one is enough to
/// have the server read again.
const QUEUE: usize = 32;

/// Which listener a route is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Reach {
    /// `127.0.0.1`, for a forwarder on this machine.
    Loopback,
    /// The private-network address from the node's settings.
    Private(SocketAddr),
}

struct Route {
    secret: SecretString,
    events: mpsc::Sender<String>,
}

type Routes = Arc<Mutex<HashMap<String, Route>>>;

/// A bound listener and the task serving it.
struct Bound {
    addr: SocketAddr,
    _serving: AbortOnDropHandle<()>,
}

/// The node's webhook listeners, bound on first use and kept for the life
/// of the node.
#[derive(Default)]
pub(super) struct Listeners {
    routes: Routes,
    bound: crate::AsyncMutex<Vec<(Reach, Bound)>>,
}

/// One watch's route. Dropping it removes the route.
pub(super) struct RouteGuard {
    token: String,
    routes: Routes,
    /// The URL the host or forwarder posts to.
    pub(super) url: String,
    /// The names of the events whose signature checked out.
    pub(super) events: mpsc::Receiver<String>,
}

impl Drop for RouteGuard {
    fn drop(&mut self) {
        lock(&self.routes).remove(&self.token);
    }
}

fn lock(routes: &Routes) -> std::sync::MutexGuard<'_, HashMap<String, Route>> {
    routes.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Listeners {
    /// A new route on the listener `reach`, binding it first if needed.
    pub(super) async fn route(&self, reach: Reach, secret: SecretString) -> Result<RouteGuard> {
        let addr = self.bind(reach).await?;
        let token = Uuid::new_v4().simple().to_string();
        let (tx, rx) = mpsc::channel(QUEUE);
        lock(&self.routes).insert(token.clone(), Route { secret, events: tx });
        Ok(RouteGuard {
            url: format!("http://{addr}{PREFIX}{token}"),
            token,
            routes: self.routes.clone(),
            events: rx,
        })
    }

    async fn bind(&self, reach: Reach) -> Result<SocketAddr> {
        let mut bound = self.bound.lock().await;
        if let Some((_, listener)) = bound.iter().find(|(at, _)| *at == reach) {
            return Ok(listener.addr);
        }
        let wanted = match reach {
            Reach::Loopback => SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            Reach::Private(addr) => addr,
        };
        let listener = TcpListener::bind(wanted)
            .await
            .with_context(|| format!("listening for webhooks on {wanted}"))?;
        let addr = listener.local_addr()?;
        log::info!("listening for webhooks on {addr}");
        let serving = AbortOnDropHandle::new(tokio::spawn(serve(listener, self.routes.clone())));
        bound.push((
            reach,
            Bound {
                addr,
                _serving: serving,
            },
        ));
        Ok(addr)
    }
}

/// Serves connections until aborted, which stops the ones open too.
async fn serve(listener: TcpListener, routes: Routes) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let stream = match accepted {
                    Ok((stream, _)) => stream,
                    Err(e) => {
                        log::debug!("a webhook connection failed: {e}");
                        continue;
                    }
                };
                // Full: the connection is dropped, and the host retries or
                // the next poll catches up.
                if connections.len() >= MAX_CONNECTIONS {
                    continue;
                }
                let routes = routes.clone();
                connections.spawn(async move {
                    if let Err(e) = answer(stream, &routes).await {
                        log::debug!("a webhook delivery failed: {e:#}");
                    }
                });
            }
            Some(_) = connections.join_next() => {}
        }
    }
}

/// What a request was answered with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Accepted,
    BadRequest,
    Unauthorized,
    NotFound,
    TooLarge,
}

impl Status {
    fn line(self) -> &'static str {
        match self {
            Self::Accepted => "204 No Content",
            Self::BadRequest => "400 Bad Request",
            Self::Unauthorized => "401 Unauthorized",
            Self::NotFound => "404 Not Found",
            Self::TooLarge => "413 Content Too Large",
        }
    }
}

async fn answer(mut stream: TcpStream, routes: &Routes) -> Result<()> {
    let status = match timeout(READ_LIMIT, read(&mut stream)).await {
        Ok(Ok(request)) => deliver(routes, &request),
        Ok(Err(status)) => status,
        Err(_) => Status::BadRequest,
    };
    let reply = format!(
        "HTTP/1.1 {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        status.line()
    );
    stream.write_all(reply.as_bytes()).await?;
    stream.shutdown().await?;
    Ok(())
}

/// One request as read.
#[derive(Debug)]
struct Request {
    path: String,
    /// Names lowercased.
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(at, _)| at == name)
            .map(|(_, value)| value.as_str())
    }
}

async fn read(stream: &mut TcpStream) -> Result<Request, Status> {
    let mut buffer = Vec::with_capacity(4096);
    let head_end = loop {
        if let Some(end) = find(&buffer, b"\r\n\r\n") {
            break end;
        }
        if buffer.len() > MAX_HEAD {
            return Err(Status::TooLarge);
        }
        let mut chunk = [0; 4096];
        let n = stream
            .read(&mut chunk)
            .await
            .map_err(|_| Status::BadRequest)?;
        if n == 0 {
            return Err(Status::BadRequest);
        }
        buffer.extend_from_slice(&chunk[..n]);
    };
    let head = std::str::from_utf8(&buffer[..head_end]).map_err(|_| Status::BadRequest)?;
    let request = parse_head(head)?;
    let length: usize = request
        .header("content-length")
        .and_then(|length| length.trim().parse().ok())
        .ok_or(Status::BadRequest)?;
    if length > MAX_BODY {
        return Err(Status::TooLarge);
    }
    let mut body = buffer.split_off(head_end + 4);
    if body.len() < length {
        let start = body.len();
        body.resize(length, 0);
        stream
            .read_exact(&mut body[start..])
            .await
            .map_err(|_| Status::BadRequest)?;
    }
    body.truncate(length);
    Ok(Request { body, ..request })
}

/// The path and headers of a POST; anything else is refused.
fn parse_head(head: &str) -> Result<Request, Status> {
    let mut lines = head.split("\r\n");
    let mut first = lines.next().ok_or(Status::BadRequest)?.split(' ');
    if first.next() != Some("POST") {
        return Err(Status::BadRequest);
    }
    let path = first.next().ok_or(Status::BadRequest)?.to_owned();
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    Ok(Request {
        path,
        headers,
        body: Vec::new(),
    })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Passes `request` on to its route when the signature checks out.
fn deliver(routes: &Routes, request: &Request) -> Status {
    let Some(token) = request.path.strip_prefix(PREFIX) else {
        return Status::NotFound;
    };
    let routes = lock(routes);
    let Some(route) = routes.get(token) else {
        return Status::NotFound;
    };
    if !signature::verify(&route.secret, &request.headers, &request.body) {
        log::info!("refused a webhook delivery with a bad signature");
        return Status::Unauthorized;
    }
    let Some(event) = event_name(request) else {
        return Status::BadRequest;
    };
    // Full: the server reads again for the queued ones anyway.
    if route.events.try_send(event).is_err() {
        log::debug!("dropped a webhook event: the queue is full");
    }
    Status::Accepted
}

/// The event a delivery is about, from GitHub's, Forgejo's or Gitea's
/// header: a short name of lowercase letters and underscores.
fn event_name(request: &Request) -> Option<String> {
    let event = ["x-github-event", "x-forgejo-event", "x-gitea-event"]
        .iter()
        .find_map(|name| request.header(name))?;
    let plain = !event.is_empty()
        && event.len() <= MAX_EVENT
        && event.bytes().all(|b| b.is_ascii_lowercase() || b == b'_');
    plain.then(|| event.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    async fn post(url: &str, headers: &[(&str, String)], body: &str) -> String {
        let rest = url.strip_prefix("http://").expect("an http URL");
        let (addr, path) = rest.split_at(rest.find('/').expect("a path"));
        let mut stream = TcpStream::connect(addr).await.expect("connects");
        let mut request = format!(
            "POST {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Length: {}\r\n",
            body.len()
        );
        for (name, value) in headers {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
        request.push_str("\r\n");
        request.push_str(body);
        stream.write_all(request.as_bytes()).await.expect("sends");
        let mut reply = String::new();
        stream.read_to_string(&mut reply).await.expect("reads");
        reply.lines().next().unwrap_or_default().to_owned()
    }

    #[tokio::test]
    async fn only_signed_deliveries_reach_the_watch() {
        let listeners = Listeners::default();
        let mut route = listeners
            .route(Reach::Loopback, SecretString::from("hook-secret"))
            .await
            .expect("binds the loopback");
        let body = r#"{"action":"opened"}"#;
        let good = format!("sha256={}", signature::sign("hook-secret", body.as_bytes()));
        let bad = format!("sha256={}", signature::sign("guess", body.as_bytes()));

        let refused = post(
            &route.url,
            &[
                ("X-GitHub-Event", "pull_request".into()),
                ("X-Hub-Signature-256", bad),
            ],
            body,
        )
        .await;
        assert_eq!(refused, "HTTP/1.1 401 Unauthorized");
        assert!(route.events.try_recv().is_err(), "nothing passed on");

        let taken = post(
            &route.url,
            &[
                ("X-GitHub-Event", "pull_request".into()),
                ("X-Hub-Signature-256", good.clone()),
            ],
            body,
        )
        .await;
        assert_eq!(taken, "HTTP/1.1 204 No Content");
        assert_eq!(route.events.recv().await.as_deref(), Some("pull_request"));

        // Another route's token, or the route once dropped, finds nothing.
        let url = route.url.clone();
        drop(route);
        let gone = post(
            &url,
            &[
                ("X-GitHub-Event", "push".into()),
                ("X-Hub-Signature-256", good),
            ],
            body,
        )
        .await;
        assert_eq!(gone, "HTTP/1.1 404 Not Found");
    }

    #[test]
    fn only_plain_event_names_pass() {
        let request = |event: &str| Request {
            path: String::new(),
            headers: vec![("x-gitea-event".into(), event.into())],
            body: Vec::new(),
        };
        assert_eq!(
            event_name(&request("workflow_run")).as_deref(),
            Some("workflow_run")
        );
        assert_eq!(event_name(&request("push\r\nX: y")), None);
        assert_eq!(event_name(&request("")), None);
    }
}
