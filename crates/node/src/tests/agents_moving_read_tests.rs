use super::saved::{Folder, Outgoing, Phase, Saved};
use super::{
    progress::Moves,
    read::{ALPN, Access, Leaving, Pulled, pull},
    worktree::MovingWorktree,
};
use crate::agents::{Parts, reopen::Reopen};
use crate::{agents::Agents, membership::Membership};
use beep_proto::SessionId;
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
struct Open(Arc<Moves>, Option<Outgoing>);

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
        let outgoing = |_: &SessionId| self.1;
        let access = Access {
            check: &|_, _| true,
            updates: None,
            leaving: &|_| None,
            outgoing: self.1.is_some().then_some(&outgoing),
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
    let agents = Agents::new(Parts {
        occupancy: crate::occupancy::Occupancy::over(
            Default::default(),
            crate::logs::found::Stores::at(Some(dir.path()), None, None),
        ),
        ..Parts::default()
    });
    // The sessions open before the last stop run again: none were open.
    agents.reopen.reopened.send_replace(true);
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
            id: &SessionId::from(id),
            copy,
        },
    )
    .await
}

fn serving(agents: &Arc<Agents>) -> impl std::future::Future<Output = Router> {
    serving_move(agents, None)
}

fn serving_move(
    agents: &Arc<Agents>,
    outgoing: Option<Outgoing>,
) -> impl std::future::Future<Output = Router> {
    serving_over(agents.moves.clone(), outgoing)
}

fn serving_over(
    moves: Arc<Moves>,
    outgoing: Option<Outgoing>,
) -> impl std::future::Future<Output = Router> {
    let handler = Open(moves, outgoing);
    async move {
        let endpoint = endpoint().await;
        Router::builder(endpoint).accept(ALPN, handler).spawn()
    }
}

#[tokio::test]
async fn a_replacement_target_cannot_take_a_session_the_previous_target_may_run() {
    let source = source(&header(""));
    let id = SessionId::from(ID);
    let folder = Folder::new(source._dir.path());
    source.agents.moves.keep_in(folder.clone());
    let mut old = Saved::over(1, Phase::Closed, None);
    old.destination = Some(SecretKey::generate().public());
    folder.save(&id, &old).expect("the source saved its close");

    let next = Outgoing {
        at: 2,
        to: SecretKey::generate().public(),
    };
    let server = serving_move(&source.agents, Some(next)).await;
    let copy = source._dir.path().join("copy.jsonl");

    let pulled = read_from(&server, &copy, ID)
        .await
        .expect("the transcript was copied");
    assert!(
        pulled.live,
        "the next target must wait while the old target settles"
    );

    old.phase = Phase::Done;
    folder
        .save(&id, &old)
        .expect("the source saved the old move's outcome");
    assert!(read_from(&server, &copy, ID).await.is_err());

    folder.clear(&id, old.at);
    let pulled = read_from(&server, &copy, ID)
        .await
        .expect("the old move was cleared");
    assert!(!pulled.live);
    server.shutdown().await.expect("the test router shuts down");
}

#[tokio::test]
async fn a_read_is_answered_from_the_owners_the_moves_hold() {
    let source = source(&header(""));
    let own = Arc::new(Reopen {
        sessions: source.agents.sessions.clone(),
        links: source.agents.links.clone(),
        ..Default::default()
    });
    let moves = Arc::new(Moves::new(
        source.agents.sessions.clone(),
        own,
        source.agents.listings.clone(),
    ));
    let server = serving_over(moves, None).await;
    let mine = TempDir::new().expect("a temp folder");
    let copy = mine.path().join("copy.jsonl");

    assert!(
        read_from(&server, &copy, ID).await.expect("a read").live,
        "the moves' own reopen has not finished, so the source may still append"
    );
    server.shutdown().await.expect("the test router shuts down");
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
    assert!(!first.live);
    assert_eq!(
        first.file_name,
        source.rollout.file_name().unwrap().to_str().unwrap()
    );

    let before = fs::read(&copy).expect("copy");
    let mut grown = before.clone();
    grown.extend_from_slice(b"{\"b\":2}\n");
    fs::write(&source.rollout, &grown).expect("appended");
    let second = read_from(&server, &copy, ID).await.expect("a second read");
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

    read_from(&server, &copy, ID).await.expect("a read");
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
    let handler =
        crate::agents::SessionsAlpn::new(source.agents.moves.clone(), Membership::default());
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
            leaving: &|_| None,
            outgoing: None,
        };
        let _ = super::read::serve(tx, rx, &self.0.moves, "laptop", access).await;
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
        worktrees: Vec::new(),
        conversation: None,
        stop: false,
        at: 1,
        worktree: None,
    };
    // The source serves its target.
    assert!(permits(
        Some(&record),
        a,
        b,
        Asked::Read,
        &SessionId::from(ID)
    ));
    // Nobody else reads, a member of the mesh or not, and not the other way.
    assert!(!permits(
        Some(&record),
        a,
        c,
        Asked::Read,
        &SessionId::from(ID)
    ));
    assert!(!permits(
        Some(&record),
        b,
        a,
        Asked::Read,
        &SessionId::from(ID)
    ));
    assert!(!permits(None, a, b, Asked::Read, &SessionId::from(ID)));
    assert!(!permits(
        Some(&record),
        a,
        b,
        Asked::Read,
        &SessionId::from("another-session")
    ));
    // The target answers its source.
    let state = Asked::State(record.at);
    assert!(permits(Some(&record), b, a, state, &SessionId::from(ID)));
    assert!(!permits(Some(&record), b, c, state, &SessionId::from(ID)));
    assert!(!permits(Some(&record), a, b, state, &SessionId::from(ID)));
    assert!(!permits(
        Some(&record),
        b,
        a,
        Asked::State(record.at + 1),
        &SessionId::from(ID)
    ));
}

#[tokio::test]
async fn a_session_that_may_still_be_coming_back_from_a_restart_reads_as_running() {
    let source = source(&header(""));
    let server = serving(&source.agents).await;
    let mine = TempDir::new().expect("a temp folder");
    let copy = mine.path().join("copy.jsonl");
    source.agents.reopen.reopened.send_replace(false);
    let pulled = read_from(&server, &copy, ID).await.expect("a read");
    assert!(pulled.live, "the source may still append when it reopens");
    source.agents.reopen.reopened.send_replace(true);
    let pulled = read_from(&server, &copy, ID).await.expect("a read");
    assert!(!pulled.live);
}

#[tokio::test]
async fn a_session_the_source_is_finishing_with_reads_as_running_until_it_lets_go() {
    let source = source(&header(""));
    let server = serving(&source.agents).await;
    let mine = TempDir::new().expect("a temp folder");
    let copy = mine.path().join("copy.jsonl");
    let held = source.agents.moves.hold(&SessionId::from(ID));
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
        id: &SessionId::from(ID),
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
    let agents = Agents::new(Parts {
        occupancy: crate::occupancy::Occupancy::over(
            Default::default(),
            crate::logs::found::Stores::at(None, None, Some(dir.path())),
        ),
        ..Parts::default()
    });
    agents.reopen.reopened.send_replace(true);
    (dir, Arc::new(agents))
}

const FLSH: &str = include_str!("../logs/found/fixtures/flsh.jsonl");

async fn read_flsh(server: &Router, copy: &Path, id: &str) -> anyhow::Result<Pulled> {
    let client = endpoint().await;
    let connection = client.connect(server.endpoint().addr(), ALPN).await?;
    let (send, recv) = connection.open_bi().await?;
    let (tx, rx) = link(send, recv);
    let ask = super::read::Ask {
        agent: AgentSlug::from("flsh"),
        id: &SessionId::from(id),
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

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// A project folder with a worktree the session made (`feat`, a commit
/// and an untracked file ahead of main) and one it did not (`other`).
fn project() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().expect("a temp folder");
    let main = dir.path().join("app");
    fs::create_dir(&main).expect("a folder");
    git(&main, &["init", "-q", "--initial-branch=main"]);
    fs::write(main.join("a.txt"), "one\n").expect("a file");
    git(&main, &["add", "."]);
    git(&main, &["commit", "-q", "-m", "first"]);
    git(
        &main,
        &["worktree", "add", "-q", "-b", "feat", ".worktrees/feat"],
    );
    git(
        &main,
        &["worktree", "add", "-q", "-b", "other", ".worktrees/other"],
    );
    let feat = main.join(".worktrees/feat");
    fs::write(feat.join("b.txt"), "two\n").expect("a file");
    git(&feat, &["add", "b.txt"]);
    git(&feat, &["commit", "-q", "-m", "on feat"]);
    fs::write(feat.join("new.txt"), "untracked\n").expect("a file");
    (dir, main)
}

/// Serves a source whose session made `feat`, a worktree since removed and
/// one outside the project.
#[derive(Clone)]
struct Project(Arc<Agents>, std::path::PathBuf);

impl std::fmt::Debug for Project {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Project")
    }
}

impl ProtocolHandler for Project {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let Ok((send, recv)) = connection.accept_bi().await else {
            return Ok(());
        };
        let (tx, rx) = link(send, recv);
        let main = self.1.clone();
        let leaving = move |_: &SessionId| {
            Some(Leaving::Project {
                folder: main.clone(),
                worktrees: vec![
                    main.join(".worktrees/feat").to_string_lossy().into_owned(),
                    main.join(".worktrees/gone").to_string_lossy().into_owned(),
                    "/elsewhere/wt".to_owned(),
                ],
            })
        };
        let access = Access {
            check: &|_, _| true,
            updates: None,
            leaving: &leaving,
            outgoing: None,
        };
        let _ = super::read::serve(tx, rx, &self.0.moves, "laptop", access).await;
        connection.closed().await;
        Ok(())
    }
}

async fn project_server() -> (TempDir, std::path::PathBuf, Router) {
    project_server_reopened(true).await
}

/// `project_server`, with the sessions from before a restart back or not.
async fn project_server_reopened(reopened: bool) -> (TempDir, std::path::PathBuf, Router) {
    let (dir, main) = project();
    let agents = Agents::default();
    agents.reopen.reopened.send_replace(reopened);
    let endpoint = endpoint().await;
    let router = Router::builder(endpoint)
        .accept(ALPN, Project(Arc::new(agents), main.clone()))
        .spawn();
    (dir, main, router)
}

async fn stream(server: &Router) -> (beep_proto::node::LinkTx, beep_proto::node::LinkRx) {
    let client = endpoint().await;
    let connection = client
        .connect(server.endpoint().addr(), ALPN)
        .await
        .expect("connects");
    let (send, recv) = connection.open_bi().await.expect("a stream");
    // The connection lives as long as the client endpoint the stream holds.
    std::mem::forget(client);
    link(send, recv)
}

#[tokio::test]
async fn the_checkout_lists_only_the_sessions_worktrees_that_are_still_there() {
    let (_dir, main, server) = project_server().await;
    let (tx, rx) = stream(&server).await;
    let checkout = super::read::checkout(tx, rx, &SessionId::from(ID))
        .await
        .expect("an answer");
    assert_eq!(checkout.head, Some(git(&main, &["rev-parse", "HEAD"])));
    assert_eq!(
        checkout.worktrees,
        vec![MovingWorktree {
            path: ".worktrees/feat".into(),
            branch: "feat".into(),
            head: git(&main.join(".worktrees/feat"), &["rev-parse", "HEAD"]),
        }]
    );
}

#[tokio::test]
async fn a_worktree_comes_with_its_new_commits_and_uncommitted_work() {
    let (_dir, main, server) = project_server().await;
    let (tx, rx) = stream(&server).await;
    let base = git(&main, &["rev-parse", "HEAD"]);
    let ask = super::read::WorktreeAsk {
        id: &SessionId::from(ID),
        path: ".worktrees/feat",
        base: &base,
        bundle: true,
    };
    let fetched = super::read::worktree(tx, rx, &ask)
        .await
        .expect("the worktree");
    assert_eq!(
        fetched.work.head,
        git(&main.join(".worktrees/feat"), &["rev-parse", "HEAD"])
    );
    assert!(fetched.bundle.is_some_and(|bytes| !bytes.is_empty()));
    assert!(String::from_utf8_lossy(&fetched.work.patch).contains("new.txt"));
}

#[tokio::test]
async fn a_worktree_the_session_did_not_make_is_refused() {
    let (_dir, _main, server) = project_server().await;
    let (tx, rx) = stream(&server).await;
    let ask = super::read::WorktreeAsk {
        id: &SessionId::from(ID),
        path: ".worktrees/other",
        base: "",
        bundle: false,
    };
    let refused = super::read::worktree(tx, rx, &ask)
        .await
        .expect_err("not the session's");
    assert!(format!("{refused:#}").contains("refused"), "{refused:#}");
}

#[tokio::test(start_paused = true)]
async fn a_worktree_asked_while_the_source_restarts_is_not_refused_for_good() {
    let (_dir, _main, server) = project_server_reopened(false).await;
    let (tx, rx) = stream(&server).await;
    let ask = super::read::WorktreeAsk {
        id: &SessionId::from(ID),
        path: ".worktrees/feat",
        base: "",
        bundle: false,
    };
    let failed = super::read::worktree(tx, rx, &ask)
        .await
        .expect_err("no worktree while the sessions come back");
    assert!(
        !failed.is::<super::read::Refused>(),
        "a refusal ends the move: {failed:#}"
    );
}

/// Serves reads to anyone, of a session leaving the folder it holds.
#[derive(Clone)]
struct Leaves(Arc<Agents>, std::path::PathBuf);

impl std::fmt::Debug for Leaves {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Leaves")
    }
}

impl ProtocolHandler for Leaves {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let Ok((send, recv)) = connection.accept_bi().await else {
            return Ok(());
        };
        let (tx, rx) = link(send, recv);
        let folder = self.1.clone();
        let leaving = move |_: &SessionId| {
            Some(Leaving::Project {
                folder: folder.clone(),
                worktrees: Vec::new(),
            })
        };
        let access = Access {
            check: &|_, _| true,
            updates: None,
            leaving: &leaving,
            outgoing: None,
        };
        let _ = super::read::serve(tx, rx, &self.0.moves, "laptop", access).await;
        connection.closed().await;
        Ok(())
    }
}

#[tokio::test]
async fn the_source_sends_what_it_saved_of_the_session_with_its_checkout() {
    use crate::agents::SavedSession;
    use beep_proto::node::agent::SessionInfo;

    let source = source("");
    source.agents.reopen.keep_saved(vec![SavedSession {
        info: SessionInfo {
            id: ID.into(),
            title: Some("Plan the week".into()),
            assistant: true,
            ..SessionInfo::default()
        },
        effort: None,
        fast: None,
        account: None,
        named: false,
    }]);
    let folder = TempDir::new().expect("a temp folder");
    let server = Router::builder(endpoint().await)
        .accept(
            ALPN,
            Leaves(source.agents.clone(), folder.path().to_owned()),
        )
        .spawn();
    let client = endpoint().await;
    let connection = client
        .connect(server.endpoint().addr(), ALPN)
        .await
        .expect("connects");
    let (send, recv) = connection.open_bi().await.expect("a stream");
    let (tx, rx) = link(send, recv);

    let checkout = super::read::checkout(tx, rx, &SessionId::from(ID))
        .await
        .expect("answered");
    let saved = checkout.session.expect("the saved session came along");
    assert_eq!(saved.info.title.as_deref(), Some("Plan the week"));
    assert!(saved.info.assistant);
}

/// Serves reads to anyone, of a chat's session leaving the folder it holds
/// while a thread brings its own.
#[derive(Clone)]
struct Chat(Arc<Agents>, std::path::PathBuf);

impl std::fmt::Debug for Chat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Chat")
    }
}

impl ProtocolHandler for Chat {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let Ok((send, recv)) = connection.accept_bi().await else {
            return Ok(());
        };
        let (tx, rx) = link(send, recv);
        let folder = self.1.clone();
        let leaving = move |_: &SessionId| {
            Some(Leaving::Chat {
                folder: folder.clone(),
                apart: vec![folder.join("research")],
            })
        };
        let access = Access {
            check: &|_, _| true,
            updates: None,
            leaving: &leaving,
            outgoing: None,
        };
        let _ = super::read::serve(tx, rx, &self.0.moves, "laptop", access).await;
        connection.closed().await;
        Ok(())
    }
}

#[tokio::test]
async fn a_chats_files_come_without_the_folder_its_thread_brings() {
    let source = source("");
    let dir = TempDir::new().expect("a temp folder");
    let chat = dir.path().join("ab12");
    fs::create_dir_all(chat.join("research")).unwrap();
    fs::write(chat.join("notes.md"), "open: pricing").unwrap();
    fs::write(chat.join("research/found.md"), "the thread's").unwrap();
    // More than one message carries.
    fs::write(chat.join("big.bin"), vec![7u8; 600 * 1024]).unwrap();
    let server = Router::builder(endpoint().await)
        .accept(ALPN, Chat(source.agents.clone(), chat))
        .spawn();
    let client = endpoint().await;
    let connection = client
        .connect(server.endpoint().addr(), ALPN)
        .await
        .expect("connects");
    let (send, recv) = connection.open_bi().await.expect("a stream");
    let (tx, rx) = link(send, recv);

    let archive = dir.path().join("files.tar");
    let mut came = Vec::new();
    super::read::files(tx, rx, &SessionId::from(ID), &archive, |held| {
        came.push(held)
    })
    .await
    .expect("the files came");
    // The bytes are counted as they come, for the move's progress.
    assert!(came.len() > 1, "{came:?}");
    assert_eq!(
        came.last().copied(),
        Some(fs::metadata(&archive).unwrap().len())
    );
    let there = dir.path().join("there");
    super::files::unpack(fs::File::open(&archive).unwrap(), &there).expect("it unpacks");

    assert_eq!(
        fs::read_to_string(there.join("notes.md")).unwrap(),
        "open: pricing"
    );
    assert_eq!(fs::read(there.join("big.bin")).unwrap().len(), 600 * 1024);
    assert!(!there.join("research").exists());
}

#[tokio::test]
async fn a_chats_checkout_says_how_much_its_files_take() {
    let source = source("");
    let dir = TempDir::new().expect("a temp folder");
    let chat = dir.path().join("ab12");
    fs::create_dir_all(chat.join("research")).unwrap();
    fs::write(chat.join("notes.md"), "open: pricing").unwrap();
    fs::write(chat.join("research/found.md"), "the thread's").unwrap();
    let server = Router::builder(endpoint().await)
        .accept(ALPN, Chat(source.agents.clone(), chat.clone()))
        .spawn();
    let client = endpoint().await;
    let connection = client
        .connect(server.endpoint().addr(), ALPN)
        .await
        .expect("connects");
    let (send, recv) = connection.open_bi().await.expect("a stream");
    let (tx, rx) = link(send, recv);

    let checkout = super::read::checkout(tx, rx, &SessionId::from(ID))
        .await
        .expect("answered");
    let size = super::files::packed_size(&chat, &[chat.join("research")]).expect("it measures");
    assert!(size > 0);
    assert_eq!(checkout.files, size);
}

#[tokio::test]
async fn a_project_sessions_folder_is_not_sent_as_a_chats_files() {
    let source = source("");
    let folder = TempDir::new().expect("a temp folder");
    fs::write(folder.path().join("secret.env"), "TOKEN=example").unwrap();
    let server = Router::builder(endpoint().await)
        .accept(
            ALPN,
            Leaves(source.agents.clone(), folder.path().to_owned()),
        )
        .spawn();
    let client = endpoint().await;
    let connection = client
        .connect(server.endpoint().addr(), ALPN)
        .await
        .expect("connects");
    let (send, recv) = connection.open_bi().await.expect("a stream");
    let (tx, rx) = link(send, recv);

    let archive = folder.path().join("files.tar");
    let refused = super::read::files(tx, rx, &SessionId::from(ID), &archive, |_| {})
        .await
        .expect_err("a project is no chat");
    assert!(refused.is::<super::read::Refused>(), "{refused:#}");
}

async fn stream_to(
    server: &Router,
) -> (
    beep_proto::node::LinkTx,
    beep_proto::node::LinkRx,
    (Endpoint, Connection),
) {
    let client = endpoint().await;
    let connection = client
        .connect(server.endpoint().addr(), ALPN)
        .await
        .expect("connects");
    let (send, recv) = connection.open_bi().await.expect("a stream");
    let (tx, rx) = link(send, recv);
    (tx, rx, (client, connection))
}

fn state(taken: bool, starting: bool, landed: bool) -> super::read::Reply {
    super::read::Reply::State {
        taken,
        failed: None,
        landed,
        starting,
    }
}

#[tokio::test]
async fn a_followed_move_pushes_each_state_on_one_stream() {
    use super::read::{Reply, Request};
    use beep_proto::node::agent::MovePhase;

    let source = source(&header(""));
    let id = SessionId::from(ID);
    let tracker = source
        .agents
        .moves
        .begin(&id, 7, SecretKey::generate().public());
    let server = serving(&source.agents).await;
    let (mut tx, mut rx, _connection) = stream_to(&server).await;
    tx.send(&Request::Follow { id, at: 7 }).await.expect("sent");

    assert_eq!(
        rx.recv::<Reply>().await.expect("a reply"),
        Some(state(false, false, false))
    );
    tracker.take();
    assert_eq!(
        rx.recv::<Reply>().await.expect("a reply"),
        Some(state(true, false, false))
    );
    assert!(tracker.resume());
    assert_eq!(
        rx.recv::<Reply>().await.expect("a reply"),
        Some(state(true, true, false))
    );
    tracker.finish(MovePhase::Done, None);
    assert_eq!(
        rx.recv::<Reply>().await.expect("a reply"),
        Some(state(true, false, true))
    );
    assert_eq!(rx.recv::<Reply>().await.expect("the end"), None);
    server.shutdown().await.expect("the test router shuts down");
}

#[tokio::test]
async fn a_source_that_follows_says_on_the_stream_when_it_is_done_with_the_session() {
    let source = source(&header("{\"a\":1}\n"));
    let held = source.agents.moves.hold(&SessionId::from(ID));
    let server = serving(&source.agents).await;
    let (tx, rx, _connection) = stream_to(&server).await;
    let mine = TempDir::new().expect("a temp folder");
    let copy = mine.path().join("copy.jsonl");
    let ask = super::read::Ask {
        agent: AgentSlug::CODEX,
        id: &SessionId::from(ID),
        copy: &copy,
    };
    let (told, heard) = tokio::sync::oneshot::channel();
    let follow = super::read::pull_following(tx, rx, ask, move |pulled| {
        let _ = told.send(pulled.live);
    });
    let release = async {
        assert!(
            heard.await.expect("told"),
            "the session still ran when it was read"
        );
        drop(held);
    };
    let (pulled, ()) = tokio::join!(follow, release);
    let pulled = pulled.expect("a read");

    assert!(pulled.live && pulled.ended);
    server.shutdown().await.expect("the test router shuts down");
}
