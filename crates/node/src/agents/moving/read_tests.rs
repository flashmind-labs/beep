use super::read::{ALPN, Pulled, pull};
use crate::{
    agents::{Agents, found::Stores},
    link::Membership,
};
use beep_proto::{AgentSlug, node::link};
use iroh::{
    Endpoint, SecretKey,
    endpoint::{Connection, presets},
    protocol::{AcceptError, ProtocolHandler, Router},
};
use std::{fs, path::Path, sync::Arc};
use tempfile::TempDir;

const ID: &str = "0b1c2d3e-0000-4000-8000-0123456789ab";

/// Serves reads with no check of who asks.
#[derive(Clone)]
struct Open(Arc<Agents>);

impl std::fmt::Debug for Open {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Open")
    }
}

impl ProtocolHandler for Open {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let Ok((send, recv)) = connection.accept_bi().await else {
            return Ok(());
        };
        let (tx, rx) = link(send, recv);
        let _ = super::read::serve(tx, rx, &self.0).await;
        connection.closed().await;
        Ok(())
    }
}

struct Source {
    _dir: TempDir,
    rollout: std::path::PathBuf,
    agents: Arc<Agents>,
}

fn source(content: &str) -> Source {
    let dir = TempDir::new().expect("a temp folder");
    let day = dir.path().join("sessions/2026/11/04");
    fs::create_dir_all(&day).expect("folders");
    let rollout = day.join(format!("rollout-2026-11-04T09-30-00-{ID}.jsonl"));
    fs::write(&rollout, content).expect("a rollout");
    let agents = Agents {
        stores: Stores::at(Some(dir.path()), None, None),
        ..Agents::default()
    };
    Source {
        _dir: dir,
        rollout,
        agents: Arc::new(agents),
    }
}

fn header(more: &str) -> String {
    format!(
        "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{ID}\",\"cwd\":\"/srv/app\"}}}}\n{more}"
    )
}

async fn endpoint() -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .bind()
        .await
        .expect("an endpoint binds")
}

async fn read_from(server: &Router, copy: &Path, id: &str) -> anyhow::Result<Pulled> {
    let client = endpoint().await;
    let connection = client.connect(server.endpoint().addr(), ALPN).await?;
    let (send, recv) = connection.open_bi().await?;
    let (tx, rx) = link(send, recv);
    pull(tx, rx, AgentSlug::CODEX, id, copy).await
}

fn serving(agents: &Arc<Agents>) -> impl std::future::Future<Output = Router> {
    let handler = Open(agents.clone());
    async move {
        let endpoint = endpoint().await;
        Router::builder(endpoint).accept(ALPN, handler).spawn()
    }
}

#[tokio::test]
async fn a_copy_is_brought_up_to_date_with_only_the_new_bytes() {
    let source = source(&header("{\"a\":1}\n"));
    let server = serving(&source.agents).await;
    let mine = TempDir::new().expect("a temp folder");
    let copy = mine.path().join("copy.jsonl");

    let first = read_from(&server, &copy, ID).await.expect("a first read");
    assert_eq!(
        fs::read(&copy).expect("copy"),
        fs::read(&source.rollout).unwrap()
    );
    assert!(!first.live && !first.restarted);
    assert_eq!(
        first.file_name,
        source.rollout.file_name().unwrap().to_str().unwrap()
    );

    let before = fs::read(&copy).expect("copy");
    let mut grown = before.clone();
    grown.extend_from_slice(b"{\"b\":2}\n");
    fs::write(&source.rollout, &grown).expect("appended");
    let second = read_from(&server, &copy, ID).await.expect("a second read");
    assert!(!second.restarted);
    assert_eq!(second.size, grown.len() as u64);
    assert_eq!(fs::read(&copy).expect("copy"), grown);
}

#[tokio::test]
async fn a_copy_that_is_not_a_start_of_the_source_is_kept_aside_and_read_again() {
    let source = source(&header("{\"a\":1}\n"));
    let server = serving(&source.agents).await;
    let mine = TempDir::new().expect("a temp folder");
    let copy = mine.path().join("copy.jsonl");
    fs::write(&copy, "something else entirely\n").expect("an old copy");

    let pulled = read_from(&server, &copy, ID).await.expect("a read");
    assert!(pulled.restarted);
    assert_eq!(
        fs::read(&copy).expect("copy"),
        fs::read(&source.rollout).unwrap()
    );
    assert_eq!(
        fs::read(mine.path().join("copy.jsonl.diverged")).expect("the old copy"),
        b"something else entirely\n"
    );
}

#[tokio::test]
async fn an_unknown_session_is_refused() {
    let source = source(&header(""));
    let server = serving(&source.agents).await;
    let mine = TempDir::new().expect("a temp folder");
    let copy = mine.path().join("copy.jsonl");
    let err = read_from(&server, &copy, "no-such-session")
        .await
        .expect_err("refused");
    assert!(format!("{err:#}").contains("no codex session"), "{err:#}");
    assert!(!copy.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn the_copy_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let source = source(&header(""));
    let server = serving(&source.agents).await;
    let mine = TempDir::new().expect("a temp folder");
    let copy = mine.path().join("copy.jsonl");
    read_from(&server, &copy, ID).await.expect("a read");
    let mode = fs::metadata(&copy).expect("copy").permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[tokio::test]
async fn a_device_outside_the_mesh_is_turned_away() {
    let source = source(&header(""));
    let handler = crate::agents::SessionsAlpn::new(source.agents.clone(), Membership::default());
    let server = Router::builder(endpoint().await)
        .accept(ALPN, handler)
        .spawn();
    let client = endpoint().await;
    let connection = client
        .connect(server.endpoint().addr(), ALPN)
        .await
        .expect("connects");
    let closed = connection.closed().await;
    assert!(
        format!("{closed:?}").contains("not in the mesh"),
        "{closed:?}"
    );
}
