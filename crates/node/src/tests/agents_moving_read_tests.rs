use super::read::{ALPN, Access, Pulled, pull};
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
use std::time::Duration;
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
        let access = Access {
            check: &|_, _| true,
            updates: None,
        };
        let _ = super::read::serve(tx, rx, &self.0, "laptop", access).await;
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
    // The sessions open before the last stop run again: none were open.
    agents.reopened.send_replace(true);
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
    pull(
        tx,
        rx,
        super::read::Ask {
            agent: AgentSlug::CODEX,
            id,
            copy,
        },
    )
    .await
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

/// Serves reads, and says what each says: nothing a stranger can do reads.
#[derive(Clone)]
struct Strict(Arc<Agents>);

impl std::fmt::Debug for Strict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Strict")
    }
}

impl ProtocolHandler for Strict {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let Ok((send, recv)) = connection.accept_bi().await else {
            return Ok(());
        };
        let (tx, rx) = link(send, recv);
        let access = Access {
            check: &|_, _| false,
            updates: None,
        };
        let _ = super::read::serve(tx, rx, &self.0, "laptop", access).await;
        connection.closed().await;
        Ok(())
    }
}

/// A source that sends the file name it is told to.
#[derive(Clone)]
struct Naming(&'static str);

impl std::fmt::Debug for Naming {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Naming")
    }
}

impl ProtocolHandler for Naming {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let Ok((send, recv)) = connection.accept_bi().await else {
            return Ok(());
        };
        let (mut tx, mut rx) = link(send, recv);
        let _ = rx.recv::<super::read::Request>().await;
        let _ = tx
            .send(&super::read::Reply::Head {
                from: 0,
                file_name: self.0.to_owned(),
            })
            .await;
        let _ = tx
            .send(&super::read::Reply::End {
                size: 0,
                live: false,
            })
            .await;
        connection.closed().await;
        Ok(())
    }
}

#[test]
fn only_a_bare_jsonl_name_is_a_transcript_name() {
    use super::read::valid_file_name;
    assert!(valid_file_name("rollout-2026-11-04T09-30-00-abc.jsonl"));
    assert!(valid_file_name("2026-11-04T09-30-00_abc.jsonl"));
    for bad in [
        "",
        ".jsonl",
        "../x.jsonl",
        "a/b.jsonl",
        "/etc/cron.d/x.jsonl",
        "..\\x.jsonl",
        "x.jsonl/..",
        "x.txt",
        ".hidden.jsonl",
        "x\0.jsonl",
        "..",
    ] {
        assert!(!valid_file_name(bad), "{bad:?}");
    }
}

#[tokio::test]
async fn a_source_that_names_the_transcript_outside_the_store_is_refused_and_nothing_is_written() {
    for name in [
        "../../escape.jsonl",
        "/tmp/escape.jsonl",
        "a/b.jsonl",
        "x.txt",
    ] {
        let server = Router::builder(endpoint().await)
            .accept(ALPN, Naming(name))
            .spawn();
        let mine = TempDir::new().expect("a temp folder");
        let copy = mine.path().join("copy.jsonl");
        let err = read_from(&server, &copy, ID).await.expect_err("refused");
        assert!(err.is::<super::read::Refused>(), "{name}: {err:#}");
        assert!(!copy.exists(), "{name}");
    }
}

#[tokio::test]
async fn a_device_no_move_names_reads_nothing() {
    let source = source(&header("{\"secret\":1}\n"));
    let server = Router::builder(endpoint().await)
        .accept(ALPN, Strict(source.agents.clone()))
        .spawn();
    let mine = TempDir::new().expect("a temp folder");
    let copy = mine.path().join("copy.jsonl");
    let err = read_from(&server, &copy, ID).await.expect_err("refused");
    assert!(err.is::<super::read::Refused>(), "{err:#}");
    assert!(!copy.exists());
}

#[test]
fn a_move_record_lets_only_its_two_machines_read_and_ask() {
    use super::{alpn::permits, read::Asked};
    use beep_proto::node::mesh_state::SessionMove;
    let (a, b, c) = (
        SecretKey::generate().public(),
        SecretKey::generate().public(),
        SecretKey::generate().public(),
    );
    let record = SessionMove {
        agent: AgentSlug::CODEX,
        id: ID.into(),
        from: a,
        to: b,
        project: "p".into(),
        path: String::new(),
        mode: None,
        model: None,
        stop: false,
        at: 1,
        worktree: None,
    };
    // The source serves its target.
    assert!(permits(Some(&record), a, b, Asked::Read, ID));
    // Nobody else reads, a member of the mesh or not, and not the other way.
    assert!(!permits(Some(&record), a, c, Asked::Read, ID));
    assert!(!permits(Some(&record), b, a, Asked::Read, ID));
    assert!(!permits(None, a, b, Asked::Read, ID));
    assert!(!permits(
        Some(&record),
        a,
        b,
        Asked::Read,
        "another-session"
    ));
    // The target answers its source.
    assert!(permits(Some(&record), b, a, Asked::State, ID));
    assert!(!permits(Some(&record), b, c, Asked::State, ID));
    assert!(!permits(Some(&record), a, b, Asked::State, ID));
}

#[tokio::test]
async fn a_session_that_may_still_be_coming_back_from_a_restart_reads_as_running() {
    let source = source(&header(""));
    let server = serving(&source.agents).await;
    let mine = TempDir::new().expect("a temp folder");
    let copy = mine.path().join("copy.jsonl");
    source.agents.reopened.send_replace(false);
    let pulled = read_from(&server, &copy, ID).await.expect("a read");
    assert!(pulled.live, "the source may still append when it reopens");
    source.agents.reopened.send_replace(true);
    let pulled = read_from(&server, &copy, ID).await.expect("a read");
    assert!(!pulled.live);
}

#[tokio::test]
async fn a_session_the_source_is_finishing_with_reads_as_running_until_it_lets_go() {
    let source = source(&header(""));
    let server = serving(&source.agents).await;
    let mine = TempDir::new().expect("a temp folder");
    let copy = mine.path().join("copy.jsonl");
    let held = source.agents.moves.hold(ID);
    assert!(read_from(&server, &copy, ID).await.expect("a read").live);
    drop(held);
    assert!(!read_from(&server, &copy, ID).await.expect("a read").live);
}

#[tokio::test]
async fn an_older_nodes_background_list_and_copy_are_refused() {
    let source = source(&header(""));
    let server = serving(&source.agents).await;
    let client = endpoint().await;
    let asks = [
        super::read::Request::List {},
        super::read::Request::Copy {
            agent: AgentSlug::CODEX,
            id: ID.into(),
            from: 0,
            prefix_hash: Vec::new(),
        },
    ];

    for ask in asks {
        // The test server answers one stream per connection.
        let connection = client
            .connect(server.endpoint().addr(), ALPN)
            .await
            .expect("connects");
        let (send, recv) = connection.open_bi().await.expect("a stream");
        let (mut tx, mut rx) = link(send, recv);
        tx.send(&ask).await.expect("sent");
        let reply = rx.recv::<super::read::Reply>().await.expect("an answer");
        assert!(
            matches!(reply, Some(super::read::Reply::Refused { .. })),
            "{reply:?}"
        );
    }
}

/// Takes the request and never answers, as a source whose stream stalled.
#[derive(Debug, Clone)]
struct Mute;

impl ProtocolHandler for Mute {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let Ok((send, recv)) = connection.accept_bi().await else {
            return Ok(());
        };
        let (_tx, mut rx) = link(send, recv);
        let _ = rx.recv::<super::read::Request>().await;
        connection.closed().await;
        Ok(())
    }
}

#[tokio::test]
async fn a_read_the_source_stops_answering_fails_instead_of_waiting_forever() {
    let server = Router::builder(endpoint().await).accept(ALPN, Mute).spawn();
    let mine = TempDir::new().expect("a temp folder");
    let copy = mine.path().join("copy.jsonl");
    let client = endpoint().await;
    let connection = client
        .connect(server.endpoint().addr(), ALPN)
        .await
        .expect("connects");
    let (send, recv) = connection.open_bi().await.expect("a stream");
    let (tx, rx) = link(send, recv);
    let ask = super::read::Ask {
        agent: AgentSlug::CODEX,
        id: ID,
        copy: &copy,
    };
    let quiet = Duration::from_millis(300);
    let err = tokio::time::timeout(
        Duration::from_secs(10),
        super::read::pull_within(tx, rx, ask, quiet),
    )
    .await
    .expect("the read gives up on its own")
    .expect_err("a stalled read fails");
    assert!(format!("{err:#}").contains("sent nothing"), "{err:#}");
    assert!(!err.is::<super::read::Refused>(), "it is tried again");
}

/// A flsh store with one session file and, in another folder, a database
/// from before flsh kept files.
fn flsh_source() -> (TempDir, Arc<Agents>) {
    let dir = TempDir::new().expect("a temp folder");
    let files = dir.path().join("455e16743bf22d19");
    fs::create_dir_all(&files).expect("folders");
    fs::write(files.join("a.jsonl"), FLSH).expect("a session file");
    let old = dir.path().join("0123456789abcdef");
    fs::create_dir_all(&old).expect("folders");
    let conn = rusqlite::Connection::open(old.join("sessions.db")).expect("a database");
    conn.execute_batch(
        "CREATE TABLE sessions (chat_key TEXT, entry_kind TEXT, content TEXT,
            turn_index INTEGER, created_at INTEGER);
         CREATE TABLE session_meta (chat_key TEXT PRIMARY KEY, title TEXT, model TEXT,
            working_dir TEXT, created_at INTEGER NOT NULL);
         INSERT INTO session_meta VALUES ('session_old', NULL, 'm', '/srv/app', 100);",
    )
    .expect("a database");
    let agents = Agents {
        stores: Stores::at(None, None, Some(dir.path())),
        ..Agents::default()
    };
    agents.reopened.send_replace(true);
    (dir, Arc::new(agents))
}

const FLSH: &str = include_str!("../agents/found/fixtures/flsh.jsonl");

async fn read_flsh(server: &Router, copy: &Path, id: &str) -> anyhow::Result<Pulled> {
    let client = endpoint().await;
    let connection = client.connect(server.endpoint().addr(), ALPN).await?;
    let (send, recv) = connection.open_bi().await?;
    let (tx, rx) = link(send, recv);
    let ask = super::read::Ask {
        agent: AgentSlug::from("flsh"),
        id,
        copy,
    };
    pull(tx, rx, ask).await
}

#[tokio::test]
async fn a_flsh_session_file_is_read_and_one_in_an_old_database_is_refused() {
    let (_dir, agents) = flsh_source();
    let server = serving(&agents).await;
    let mine = TempDir::new().expect("a temp folder");
    let copy = mine.path().join("copy.jsonl");
    let id = "session_0b1c2d3e00004000800001234567890a";
    let pulled = read_flsh(&server, &copy, id).await.expect("a read");
    assert_eq!(pulled.file_name, "a.jsonl");
    assert_eq!(fs::read_to_string(&copy).expect("copy"), FLSH);

    let old = mine.path().join("old.jsonl");
    let err = read_flsh(&server, &old, "session_old")
        .await
        .expect_err("refused");
    assert!(err.is::<super::read::Refused>(), "{err:#}");
    assert!(
        err.to_string()
            .contains("Update flsh on laptop to 0.2.9 to move this session"),
        "{err:#}"
    );
}
