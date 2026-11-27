use super::unix::{Docker, LABEL};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response, StatusCode, body::Incoming, header, service::service_fn};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use std::{convert::Infallible, path::Path, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::mpsc,
    time::timeout,
};

/// A daemon that keeps each container definition it is sent, lists one
/// container, and echoes what an attach sends.
fn daemon(socket: &Path) -> mpsc::UnboundedReceiver<Value> {
    let listener = UnixListener::bind(socket).unwrap();
    let (created, receiver) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let created = created.clone();
            tokio::spawn(async move {
                let service = service_fn(move |request| answer(request, created.clone()));
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .with_upgrades()
                    .await;
            });
        }
    });

    receiver
}

async fn answer(
    mut request: Request<Incoming>,
    created: mpsc::UnboundedSender<Value>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let path = request.uri().path().to_owned();

    if path.ends_with("/attach") {
        let upgrade = hyper::upgrade::on(&mut request);
        tokio::spawn(async move {
            let mut io = TokioIo::new(upgrade.await.unwrap());
            let mut buf = [0; 4];
            io.read_exact(&mut buf).await.unwrap();
            io.write_all(&buf).await.unwrap();
        });
        let mut reply = Response::new(Full::default());
        *reply.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
        reply
            .headers_mut()
            .insert(header::CONNECTION, "Upgrade".parse().unwrap());
        reply
            .headers_mut()
            .insert(header::UPGRADE, "tcp".parse().unwrap());
        return Ok(reply);
    }

    let reply = match path.as_str() {
        "/v1.47/containers/create" => {
            let body = request.into_body().collect().await.unwrap().to_bytes();
            created
                .send(serde_json::from_slice(&body).unwrap())
                .unwrap();
            json!({ "Id": "c0ffee" })
        }
        "/v1.47/containers/c0ffee/start" => {
            let mut reply = Response::new(Full::default());
            *reply.status_mut() = StatusCode::NO_CONTENT;
            return Ok(reply);
        }
        "/containers/json" => {
            let query = request.uri().query().unwrap_or_default().to_owned();
            assert!(query.contains("beep.session"), "{query}");
            json!([{
                "Id": "c0ffee",
                "Names": ["/web"],
                "Image": "nginx",
                "Created": 1_700_000_000,
                "Ports": [
                    { "IP": "0.0.0.0", "PrivatePort": 80, "PublicPort": 8080, "Type": "tcp" },
                    { "IP": "::", "PrivatePort": 80, "PublicPort": 8080, "Type": "tcp" },
                    { "PrivatePort": 9000, "Type": "tcp" },
                    { "IP": "0.0.0.0", "PrivatePort": 53, "PublicPort": 5353, "Type": "udp" }
                ]
            }])
        }
        _ => json!({}),
    };

    Ok(Response::new(Full::new(Bytes::from(reply.to_string()))))
}

/// Sends one request on a connection of its own to the session's socket.
async fn send(socket: &Path, request: Request<Full<Bytes>>) -> Response<Incoming> {
    let stream = UnixStream::connect(socket).await.unwrap();
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .unwrap();
    tokio::spawn(connection.with_upgrades());
    sender.send_request(request).await.unwrap()
}

#[tokio::test]
async fn labels_what_the_agent_creates_and_lists_it_with_its_ports() {
    let dir = tempfile::tempdir().unwrap();
    let upstream = dir.path().join("docker.sock");
    let mut created = daemon(&upstream);
    let docker = Docker::listen(&dir.path().join("sessions"), upstream).unwrap();
    let socket = docker.host().strip_prefix("unix://").unwrap().to_owned();
    let socket = Path::new(&socket);

    let definition = json!({ "Image": "nginx", "Labels": { "app": "web" } }).to_string();
    let request = Request::post("/v1.47/containers/create")
        .header(header::HOST, "docker")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from(definition)))
        .unwrap();
    assert!(send(socket, request).await.status().is_success());
    let labels = &created.recv().await.unwrap()["Labels"];
    assert_eq!(labels["app"], "web");
    assert!(
        labels[LABEL]
            .as_str()
            .is_some_and(|token| token.len() == 16)
    );

    let request = Request::post("/v1.47/containers/c0ffee/start")
        .header(header::HOST, "docker")
        .body(Full::default())
        .unwrap();
    assert_eq!(send(socket, request).await.status(), StatusCode::NO_CONTENT);
    timeout(Duration::from_secs(5), docker.started())
        .await
        .unwrap();

    let containers = docker.containers().await.unwrap();
    assert_eq!(containers.len(), 1);
    assert_eq!(containers[0].name, "web");
    assert_eq!(containers[0].ports, [8080]);
}

#[tokio::test]
async fn an_attach_takes_over_the_connection_both_ways() {
    let dir = tempfile::tempdir().unwrap();
    let upstream = dir.path().join("docker.sock");
    let _created = daemon(&upstream);
    let docker = Docker::listen(&dir.path().join("sessions"), upstream).unwrap();
    let socket = docker.host().strip_prefix("unix://").unwrap().to_owned();

    let request = Request::post("/v1.47/containers/c0ffee/attach?stream=1")
        .header(header::HOST, "docker")
        .header(header::CONNECTION, "Upgrade")
        .header(header::UPGRADE, "tcp")
        .body(Full::default())
        .unwrap();
    let mut reply = send(Path::new(&socket), request).await;
    assert_eq!(reply.status(), StatusCode::SWITCHING_PROTOCOLS);

    let mut io = TokioIo::new(hyper::upgrade::on(&mut reply).await.unwrap());
    io.write_all(b"ping").await.unwrap();
    let mut echoed = [0; 4];
    timeout(Duration::from_secs(5), io.read_exact(&mut echoed))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&echoed, b"ping");
}

#[tokio::test]
async fn the_socket_goes_with_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let upstream = dir.path().join("docker.sock");
    let _created = daemon(&upstream);
    let docker = Docker::listen(&dir.path().join("sessions"), upstream).unwrap();
    let socket = docker.host().strip_prefix("unix://").unwrap().to_owned();
    assert!(Path::new(&socket).exists());

    drop(docker);
    assert!(!Path::new(&socket).exists());
}

/// Runs the Docker CLI against the session's socket, as the agent would.
async fn docker(host: &str, dir: &Path, args: &[&str]) -> std::process::Output {
    tokio::process::Command::new("docker")
        .args(args)
        .env("DOCKER_HOST", host)
        .current_dir(dir)
        .output()
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "needs a Docker daemon with the node:24-slim image"]
async fn a_real_daemon_runs_attaches_and_lists_a_compose_service() {
    let Some(session) = Docker::open(None).await else {
        panic!("no Docker daemon on a Unix socket");
    };
    let host = session.host();
    let dir = tempfile::tempdir().unwrap();

    // An attached run takes over its connection for the output.
    let ran = docker(
        &host,
        dir.path(),
        &[
            "run",
            "--rm",
            "node:24-slim",
            "node",
            "-e",
            "console.log('attached')",
        ],
    )
    .await;
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&ran.stdout).trim(), "attached");

    // A build streams its context and keeps a session open beside it.
    std::fs::write(
        dir.path().join("Dockerfile"),
        "FROM node:24-slim\nRUN echo built > /built\n",
    )
    .unwrap();
    let tag = format!("beep-test-build:{}", std::process::id());
    let built = docker(&host, dir.path(), &["build", "-q", "-t", &tag, "."]).await;
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let removed = docker(&host, dir.path(), &["image", "rm", &tag]).await;
    assert!(
        removed.status.success(),
        "{}",
        String::from_utf8_lossy(&removed.stderr)
    );

    let compose = r#"
services:
  web:
    image: node:24-slim
    command: ["node", "-e", "require('http').createServer((q, r) => r.end('hi')).listen(80)"]
    ports: ["127.0.0.1::80"]
"#;
    std::fs::write(dir.path().join("compose.yaml"), compose).unwrap();
    let project = format!("beep-test-{}", std::process::id());
    let up = docker(&host, dir.path(), &["compose", "-p", &project, "up", "-d"]).await;
    assert!(
        up.status.success(),
        "{}",
        String::from_utf8_lossy(&up.stderr)
    );
    timeout(Duration::from_secs(5), session.started())
        .await
        .unwrap();

    let containers = session.containers().await.unwrap();
    let down = docker(&host, dir.path(), &["compose", "-p", &project, "down"]).await;
    assert!(
        down.status.success(),
        "{}",
        String::from_utf8_lossy(&down.stderr)
    );
    assert_eq!(containers.len(), 1, "{containers:?}");
    assert!(containers[0].name.contains("web"));
    assert_eq!(containers[0].ports.len(), 1);
}
