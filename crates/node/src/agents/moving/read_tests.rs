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
        let _ = super::read::serve(tx, rx, &self.0, &|_, _| true, &[]).await;
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
            kind: super::read::Kind::Move { worktree: false },
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
        let _ = super::read::serve(tx, rx, &self.0, &|_, _| false, &[]).await;
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
async fn a_background_read_of_a_session_no_project_offers_is_refused() {
    let source = source(&header(""));
    let server = serving(&source.agents).await;
    let mine = TempDir::new().expect("a temp folder");
    let copy = mine.path().join("copy.jsonl");
    let client = endpoint().await;
    let connection = client
        .connect(server.endpoint().addr(), ALPN)
        .await
        .expect("connects");
    let (send, recv) = connection.open_bi().await.expect("a stream");
    let (tx, rx) = link(send, recv);
    // The server offers the sessions of no project, so no session is offered.
    let err = pull(
        tx,
        rx,
        super::read::Ask {
            agent: AgentSlug::CODEX,
            id: ID,
            copy: &copy,
            kind: super::read::Kind::Background { rate: 1 << 30 },
        },
    )
    .await
    .expect_err("refused");
    assert!(format!("{err:#}").contains("not offered"), "{err:#}");
    assert!(!copy.exists());
}

#[tokio::test]
async fn a_worktree_read_before_the_source_registered_the_worktree_reads_as_running() {
    let source = source(&header(""));
    let server = serving(&source.agents).await;
    let mine = TempDir::new().expect("a temp folder");
    let client = endpoint().await;
    let pull_with = |copy: std::path::PathBuf, worktree: bool| {
        let (client, addr) = (client.clone(), server.endpoint().addr());
        async move {
            let connection = client.connect(addr, ALPN).await.expect("connects");
            let (send, recv) = connection.open_bi().await.expect("a stream");
            let (tx, rx) = link(send, recv);
            pull(
                tx,
                rx,
                super::read::Ask {
                    agent: AgentSlug::CODEX,
                    id: ID,
                    copy: &copy,
                    kind: super::read::Kind::Move { worktree },
                },
            )
            .await
            .expect("a read")
        }
    };
    // The worktree is not registered yet: not read as ended, or the target
    // would get no commit and no work, and fail for good.
    let early = pull_with(mine.path().join("a.jsonl"), true).await;
    assert!(early.live && early.work.is_none());
    // A read of a checkout session is not held up by that.
    let plain = pull_with(mine.path().join("b.jsonl"), false).await;
    assert!(!plain.live);
}

#[cfg(unix)]
#[tokio::test]
async fn a_live_read_refuses_what_the_snapshot_cannot_read_before_the_source_stops_anything() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt, process::Command};
    let source = source(&header(""));
    let server = serving(&source.agents).await;
    let mine = TempDir::new().expect("a temp folder");
    // A worktree with a new file whose name is not UTF-8: the snapshot
    // cannot read it, and no size estimate would see that.
    let repo = TempDir::new().expect("a temp folder");
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(repo.path())
            .output()
            .expect("git runs");
        assert!(out.status.success(), "git {args:?}");
    };
    git(&["init", "-q", "--initial-branch=main"]);
    fs::write(repo.path().join("a.txt"), "one\n").expect("a file");
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "first"]);
    fs::write(repo.path().join(OsStr::from_bytes(b"bad\xff.txt")), "x\n").expect("a file");
    source
        .agents
        .moves
        .set_worktree(ID, repo.path().to_path_buf());
    // The session still runs: the read is a live one.
    let _held = source.agents.moves.hold(ID);
    let client = endpoint().await;
    let connection = client
        .connect(server.endpoint().addr(), ALPN)
        .await
        .expect("connects");
    let (send, recv) = connection.open_bi().await.expect("a stream");
    let (tx, rx) = link(send, recv);
    let copy = mine.path().join("copy.jsonl");
    let err = pull(
        tx,
        rx,
        super::read::Ask {
            agent: AgentSlug::CODEX,
            id: ID,
            copy: &copy,
            kind: super::read::Kind::Move { worktree: true },
        },
    )
    .await
    .expect_err("refused on the live read");
    assert!(err.is::<super::read::Refused>(), "{err:#}");
}
