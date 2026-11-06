//! Real `beep-node` processes with no server, throwaway state folders and
//! relays off. The test process plays the desktop: it creates the mesh,
//! mints tickets and calls the nodes over `beep-peer/1`, the way the real
//! desktop does. A node cannot create a mesh, so none of these tests could
//! start one in a node.
//!
//! Waits ride on the nodes' log lines, the mesh's `updates()` and channels,
//! each under a generous outer timeout; nothing sleeps.

use beep_mesh::{DeviceKind, JOIN_ALPN, JoinTicket, Key, Mesh, PEER_ALPN};
use beep_node::link::peers::ReportError;
use beep_proto::node::{
    Down, LinkRx, LinkTx, NODE_ALPN, NODE_ALPN_NAMED, NodeHello, Open, Opened,
    agent::{
        Event as AgentEvent, MovePhase, Request as AgentRequest, SESSIONS_PROTOCOL, SessionsReply,
        SessionsRequest, Status,
    },
    link,
    process::{Reply as ProcessReply, Request as ProcessRequest},
    report::{PROTOCOL as REPORT, Reply as ReportReply, Request as ReportRequest},
    shell::{Event as ShellEvent, PROTOCOL as SHELL, Request as ShellRequest},
    status::{PROTOCOL as STATUS, Report},
    tools::{PROTOCOL as TOOLS, Reply, Request as ToolRequest, Tool},
};
use beep_proto::{
    SecretText,
    node::{
        EnvSecret, MachineCredentials,
        harness::{AgentSettings, HarnessConfig},
        mesh_state::{Credential, Place, SessionMove, StateValue, SyncSpec},
        sync::{
            PROTOCOL as SYNC, Project as SyncProject, Reply as SyncReply, Request as SyncRequest,
        },
    },
};
use beep_report::Answer;
use futures::{SinkExt, StreamExt};
use iroh::{
    Endpoint, EndpointId, SecretKey,
    endpoint::{Connection, presets},
    protocol::{AcceptError, ProtocolHandler, Router},
};
use iroh_tickets::Ticket;
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tempfile::TempDir;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, Lines},
    net::{TcpListener, TcpStream},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::{mpsc, watch},
    task::JoinSet,
    time::timeout,
};
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};
use tokio_util::task::AbortOnDropHandle;

const LIMIT: Duration = Duration::from_secs(90);
/// What the node logs once its control socket takes commands.
const READY: &str = "taking mesh commands";

/// A desktop-side or client-side member living in the test process.
struct Member {
    mesh: Mesh,
    _router: Router,
    _dir: TempDir,
}

impl Member {
    fn id(&self) -> EndpointId {
        self._router.endpoint().id()
    }
}

/// What the test's "server" got from a node that dialed it.
#[derive(Debug)]
struct Server(mpsc::UnboundedSender<NodeHello>);

impl ProtocolHandler for Server {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let Ok((send, recv)) = connection.accept_bi().await else {
            return Ok(());
        };
        let (mut tx, mut rx) = link(send, recv);
        if let Ok(Some(hello)) = rx.recv::<NodeHello>().await {
            tx.send(&Down::Welcome { tools: false }).await.ok();
            self.0.send(hello).ok();
        }
        connection.closed().await;
        Ok(())
    }
}

async fn bind(key: &SecretKey) -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .secret_key(key.clone())
        .transport_config(beep_mesh::transport_config())
        .bind()
        .await
        .expect("an endpoint binds")
}

fn router(endpoint: Endpoint, mesh: &Mesh, server: Option<Server>) -> Router {
    let (peer, join) = mesh.protocols();
    let router = Router::builder(endpoint)
        .accept(PEER_ALPN, peer)
        .accept(JOIN_ALPN, join);
    match server {
        Some(server) => router.accept(NODE_ALPN, server),
        None => router,
    }
    .spawn()
}

async fn desktop(server: Option<Server>) -> Member {
    let key = SecretKey::generate();
    let dir = TempDir::new().expect("a temp folder");
    let endpoint = bind(&key).await;
    let mesh = Mesh::create(dir.path(), endpoint.clone(), "desktop")
        .await
        .expect("the mesh is created");
    Member {
        _router: router(endpoint, &mesh, server),
        mesh,
        _dir: dir,
    }
}

/// A node-kind member in this process, to play another node.
async fn peer_node(from: &Member) -> Member {
    let ticket = from.ticket(DeviceKind::Node).await;
    let dir = TempDir::new().expect("a temp folder");
    let endpoint = bind(&SecretKey::generate()).await;
    let mesh = Mesh::join(dir.path(), endpoint.clone(), &ticket, "other node")
        .await
        .expect("the node joins");
    Member {
        _router: router(endpoint, &mesh, None),
        mesh,
        _dir: dir,
    }
}

impl Member {
    async fn ticket(&self, kind: DeviceKind) -> JoinTicket {
        self.mesh
            .invite(kind, Duration::from_secs(3600))
            .await
            .expect("a ticket is minted")
    }

    /// Waits until `id` is online here, then returns the way to open streams to it.
    async fn reach(&self, id: EndpointId) -> Link {
        let mut updates = self.mesh.updates();
        timeout(LIMIT, async {
            loop {
                updates.borrow_and_update();
                if self
                    .mesh
                    .devices()
                    .iter()
                    .any(|d| d.device.id == id && d.online)
                    && self.mesh.peer(id).is_some()
                {
                    return Link {
                        mesh: self.mesh.clone(),
                        id,
                    };
                }
                updates.changed().await.expect("the mesh is alive");
            }
        })
        .await
        .expect("the node came online in time")
    }
}

/// A member of the mesh, reached through `Mesh::open_bi`, which follows a
/// link that replaced the one it was handed.
struct Link {
    mesh: Mesh,
    id: EndpointId,
}

/// A `beep-node run` process: its folder, its fake home and its log lines.
struct Node {
    /// Kills the process when dropped.
    _child: Child,
    id: EndpointId,
    dir: TempDir,
    home: TempDir,
    lines: mpsc::UnboundedReceiver<String>,
    /// Every line logged so far.
    log: Vec<String>,
    /// Environment the process runs with besides the test's own.
    env: Vec<(&'static str, String)>,
}

#[derive(Default)]
struct Config {
    /// Environment variables for the process.
    env: Vec<(&'static str, String)>,
    server: Option<EndpointId>,
    agent: Option<(String, Vec<String>)>,
    name: Option<&'static str>,
    /// Folders the node shares for files and sync.
    files: Vec<PathBuf>,
    /// The UDP port to bind; random when unset.
    port: Option<u16>,
    /// Project folders a server set before this start, kept in the node's
    /// saved list under their folder names.
    saved_projects: Vec<PathBuf>,
}

impl Node {
    async fn start(config: Config) -> Self {
        let dir = TempDir::new().expect("a temp folder");
        let home = TempDir::new().expect("a temp folder");
        // The agents' stores exist, as on a machine that has run them.
        for store in [".claude/projects", ".codex/sessions", ".pi/agent/sessions"] {
            fs::create_dir_all(home.path().join(store)).expect("a store folder");
        }
        let key = SecretKey::generate();
        let key_file = dir.path().join("key");
        fs::write(&key_file, key.to_bytes()).expect("the key is written");
        restrict(&key_file);
        // A random port, so no node here takes the live service's.
        let mut settings =
            json!({ "name": config.name.unwrap_or("box-a"), "port": config.port.unwrap_or(0) });
        if !config.files.is_empty() {
            settings["files"] = json!(config.files);
        }
        if let Some(server) = config.server {
            settings["server"] = json!(server.to_string());
        }
        if let Some((program, args)) = config.agent {
            settings["agents"] = json!([{ "name": "codex", "program": program, "args": args }]);
        }
        let text = toml::to_string(&toml::Value::try_from(settings).expect("settings are toml"))
            .expect("settings encode");
        fs::write(dir.path().join("node.toml"), text).expect("settings are written");
        if !config.saved_projects.is_empty() {
            let projects: Vec<_> = config
                .saved_projects
                .iter()
                .map(|folder| {
                    let name = folder.file_name().and_then(|n| n.to_str()).expect("a name");
                    json!({ "name": name, "folder": folder, "peers": [] })
                })
                .collect();
            let saved = dir.path().join("blobs/sync");
            fs::create_dir_all(&saved).expect("the saved list folder");
            fs::write(
                saved.join("projects.json"),
                json!({ "projects": projects }).to_string(),
            )
            .expect("the saved list is written");
        }
        Self::run(dir, home, key.public(), config.env).await
    }

    /// Stops the process; `Stopped::start` runs it again on the same folder.
    async fn stop(self) -> Stopped {
        let Self {
            mut _child,
            id,
            dir,
            home,
            env,
            ..
        } = self;
        _child.kill().await.expect("the node stops");
        Stopped { dir, home, id, env }
    }

    /// Stops the process and starts it again on the same folder.
    async fn restart(self) -> Self {
        let Self {
            mut _child,
            id,
            dir,
            home,
            env,
            ..
        } = self;
        _child.kill().await.expect("the node stops");
        Self::run(dir, home, id, env).await
    }

    async fn run(
        dir: TempDir,
        home: TempDir,
        id: EndpointId,
        env: Vec<(&'static str, String)>,
    ) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_beep-node"))
            .envs(env.iter().map(|(name, value)| (name, value)))
            .arg("--dir")
            .arg(dir.path())
            .arg("run")
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .env("XDG_STATE_HOME", home.path().join("state"))
            .env("XDG_DATA_HOME", home.path().join("data"))
            .env("XDG_STATE_HOME", home.path().join("state"))
            .env("BEEP_NODE_NO_RELAY", "1")
            // The agents' session stores stay in the fake home.
            .env("CLAUDE_CONFIG_DIR", home.path().join(".claude"))
            .env("CODEX_HOME", home.path().join(".codex"))
            .env(
                "PI_CODING_AGENT_SESSION_DIR",
                home.path().join(".pi/agent/sessions"),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("the node starts");
        let stderr = child.stderr.take().expect("stderr is piped");
        let (send, lines) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if send.send(line).is_err() {
                    return;
                }
            }
        });
        let mut node = Self {
            _child: child,
            id,
            dir,
            home,
            lines,
            log: Vec::new(),
            env,
        };
        node.logged(READY).await;
        node
    }

    /// Waits for a log line containing `text`.
    async fn logged(&mut self, text: &str) -> String {
        timeout(LIMIT, async {
            while let Some(line) = self.lines.recv().await {
                self.log.push(line.clone());
                if line.contains(text) {
                    return line;
                }
            }
            panic!("the node ended before logging {text:?}");
        })
        .await
        .unwrap_or_else(|_| {
            let tail: Vec<_> = self.log.iter().rev().take(40).rev().collect();
            panic!("the node did not log {text:?} in time; it logged {tail:#?}")
        })
    }

    /// `beep-node mesh join -`, with the ticket on stdin.
    async fn join(&self, ticket: &JoinTicket) {
        self.join_with(ticket, &[]).await;
    }

    /// `beep-node mesh leave`.
    async fn leave(&self) {
        let out = Command::new(env!("CARGO_BIN_EXE_beep-node"))
            .args(["mesh", "leave", "--dir"])
            .arg(self.dir.path())
            .kill_on_drop(true)
            .output();
        let out = timeout(LIMIT, out)
            .await
            .expect("the command finished in time")
            .expect("the command ran");
        assert!(
            out.status.success(),
            "leave failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// `join` with more arguments for the command.
    async fn join_with(&self, ticket: &JoinTicket, more: &[&str]) {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_beep-node"))
            .args(["mesh", "join", "-", "--dir"])
            .arg(self.dir.path())
            .args(more)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("the command starts");
        let mut stdin = cmd.stdin.take().expect("stdin is piped");
        let text = ticket.encode_string();
        stdin
            .write_all(text.as_bytes())
            .await
            .expect("stdin takes it");
        drop(stdin);
        let out = timeout(LIMIT, cmd.wait_with_output())
            .await
            .expect("the join finished in time")
            .expect("the command ran");
        let shown = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "join failed: {shown}");
        assert!(!shown.contains(&text), "the ticket was printed");
    }
}

#[cfg(unix)]
fn restrict(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).expect("the key is private");
}

#[cfg(not(unix))]
fn restrict(_: &Path) {}

struct Stream {
    tx: LinkTx,
    rx: LinkRx,
}

/// Opens `protocol` on `conn`; the node's answer comes with the stream.
async fn open(conn: &Link, protocol: &str) -> (Stream, Opened) {
    let (send, recv) = conn.mesh.open_bi(conn.id).await.expect("a stream opens");
    let (mut tx, mut rx) = link(send, recv);
    tx.send(&Open {
        protocol: protocol.to_owned(),
    })
    .await
    .expect("the open is sent");
    let opened = timeout(LIMIT, rx.recv::<Opened>())
        .await
        .expect("the node answered in time")
        .expect("the answer reads")
        .expect("the node answered");
    (Stream { tx, rx }, opened)
}

async fn ready(conn: &Link, protocol: &str) -> Stream {
    match open(conn, protocol).await {
        (stream, Opened::Ready {}) => stream,
        (_, other) => panic!("{protocol} was not opened: {other:?}"),
    }
}

async fn tool(conn: &Link, tool: Tool, args: serde_json::Value) -> Reply {
    try_tool(conn, tool, args)
        .await
        .unwrap_or_else(|e| panic!("the call failed: {e}"))
}

/// `tool`, with a failure to reach the node as an error: a link to a node
/// that restarted may be the old one for a moment.
async fn try_tool(conn: &Link, tool: Tool, args: serde_json::Value) -> Result<Reply, String> {
    let fail = |e: &dyn std::fmt::Display| e.to_string();
    let (send, recv) = conn.mesh.open_bi(conn.id).await.map_err(|e| fail(&e))?;
    let (mut tx, mut rx) = link(send, recv);
    tx.send(&Open {
        protocol: TOOLS.to_owned(),
    })
    .await
    .map_err(|e| fail(&e))?;
    match timeout(LIMIT, rx.recv::<Opened>())
        .await
        .expect("the node answered in time")
        .map_err(|e| fail(&e))?
    {
        Some(Opened::Ready {}) => {}
        other => return Err(format!("{TOOLS} was not opened: {other:?}")),
    }
    tx.send(&ToolRequest {
        session: "a3f9c1".to_owned(),
        tool,
        args: args.to_string(),
        approved: Default::default(),
    })
    .await
    .map_err(|e| fail(&e))?;
    timeout(LIMIT, rx.recv::<Reply>())
        .await
        .expect("the call answered in time")
        .map_err(|e| fail(&e))?
        .ok_or_else(|| "the node closed the call".to_owned())
}

#[tokio::test]
async fn a_peer_runs_a_command_on_a_node_with_no_server() {
    let desktop = desktop(None).await;
    let node = Node::start(Config::default()).await;
    assert!(
        node.log
            .iter()
            .any(|line| line.contains("no server and no mesh")),
        "{:?}",
        node.log
    );
    node.join(&desktop.ticket(DeviceKind::Node).await).await;
    let conn = desktop.reach(node.id).await;
    let mut shell = ready(&conn, SHELL).await;
    shell
        .tx
        .send(&ShellRequest::Run {
            command: "echo from-the-peer".to_owned(),
            timeout_secs: 30,
        })
        .await
        .expect("the run is sent");
    let mut output = String::new();
    timeout(LIMIT, async {
        loop {
            match shell.rx.recv::<ShellEvent>().await.expect("an event reads") {
                Some(ShellEvent::Output { text }) => output.push_str(&text),
                Some(ShellEvent::Done { exit_code, .. }) => {
                    assert_eq!(exit_code, Some(0));
                    return;
                }
                other => panic!("the shell ended early: {other:?}"),
            }
        }
    })
    .await
    .expect("the command finished in time");
    assert!(output.contains("from-the-peer"), "{output:?}");
}

const ADAPTER: &str = r#"
log() { printf '%s\n' "$line" >> "$1"; }
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
read -r line; log "$1"
reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line; log "$1"
reply '{"sessionId":"s"}'
read -r line; log "$1"
reply '{"stopReason":"end_turn"}'
cat > /dev/null
"#;

#[cfg(unix)]
#[tokio::test]
async fn a_peer_starts_and_follows_an_agent_session() {
    let desktop = desktop(None).await;
    let work = TempDir::new().expect("a temp folder");
    let log = work.path().join("sent.jsonl");
    let node = Node::start(Config {
        agent: Some((
            "sh".to_owned(),
            vec![
                "-c".to_owned(),
                ADAPTER.to_owned(),
                "fake-adapter".to_owned(),
                log.display().to_string(),
            ],
        )),
        ..Config::default()
    })
    .await;
    node.join(&desktop.ticket(DeviceKind::Node).await).await;
    let conn = desktop.reach(node.id).await;
    let mut agent = ready(&conn, "codex/1").await;
    agent
        .tx
        .send(&AgentRequest::Start {
            cwd: Some(work.path().display().to_string()),
            prompt: "go".to_owned(),
            model: None,
            effort: None,
            checkout: None,
        })
        .await
        .expect("the start is sent");
    let session = timeout(LIMIT, async {
        let mut id = None;
        loop {
            match agent.rx.recv::<AgentEvent>().await.expect("an event reads") {
                Some(AgentEvent::Attached { session }) => id = Some(session.id),
                Some(AgentEvent::TurnDone { .. }) => return id.expect("attached first"),
                Some(AgentEvent::Ended { reason }) => panic!("the session ended: {reason}"),
                Some(_) => {}
                None => panic!("the stream closed"),
            }
        }
    })
    .await
    .expect("the turn finished in time");
    // A second stream follows the same session.
    let mut follow = ready(&conn, "codex/1").await;
    follow
        .tx
        .send(&AgentRequest::Attach { session })
        .await
        .expect("the attach is sent");
    match timeout(LIMIT, follow.rx.recv::<AgentEvent>())
        .await
        .expect("the attach answered in time")
        .expect("an event reads")
    {
        Some(AgentEvent::Attached { session }) => assert_eq!(session.turns, 1),
        other => panic!("not attached: {other:?}"),
    }
}

#[tokio::test]
async fn a_stranger_gets_no_shell() {
    let desktop = desktop(None).await;
    let node = Node::start(Config::default()).await;
    node.join(&desktop.ticket(DeviceKind::Node).await).await;
    desktop.reach(node.id).await;
    let addr = desktop
        .mesh
        .devices()
        .into_iter()
        .find(|d| d.device.id == node.id)
        .and_then(|d| d.addr)
        .expect("the desktop knows where the node is");
    let stranger = bind(&SecretKey::generate()).await;
    for alpn in [PEER_ALPN, JOIN_ALPN] {
        let conn = stranger
            .connect(addr.clone(), alpn)
            .await
            .expect("the node accepts the connection before it decides");
        // The node closes it without serving anything.
        timeout(LIMIT, conn.closed())
            .await
            .expect("the node closed the stranger's connection");
    }
    // A member still gets in.
    assert!(desktop.mesh.peer(node.id).is_some());
}

#[tokio::test]
async fn another_node_gets_no_shell_but_can_use_agent_tools() {
    let desktop = desktop(None).await;
    let node = Node::start(Config {
        agent: Some(("sh".to_owned(), vec!["-c".to_owned(), "true".to_owned()])),
        ..Config::default()
    })
    .await;
    node.join(&desktop.ticket(DeviceKind::Node).await).await;
    let other = peer_node(&desktop).await;
    let conn = other.reach(node.id).await;
    let refused = [
        SHELL,
        "process/1",
        "agents/1",
        "files/1",
        "terminal/1",
        "codex/1",
        "desktop/1",
    ];
    for protocol in refused {
        match open(&conn, protocol).await {
            (_, Opened::Refused { .. }) => {}
            (_, other) => panic!("a node got {protocol}: {other:?}"),
        }
    }
    let Reply::Done { text, failed } = tool(
        &conn,
        Tool::RunOn,
        json!({ "machine": "box-a", "command": "echo agent-tool" }),
    )
    .await
    else {
        panic!("not done");
    };
    assert!(!failed, "{text}");
    assert!(text.contains("agent-tool"), "{text:?}");
    // The tools that are not between machines stay refused.
    let Reply::Done { failed, .. } = tool(&conn, Tool::Remember, json!({})).await else {
        panic!("not done");
    };
    assert!(failed);
    let mut report = ready(&conn, STATUS).await;
    match timeout(LIMIT, report.rx.recv::<Report>())
        .await
        .expect("the status came in time")
        .expect("a report reads")
    {
        Some(Report::Offers(offers)) => assert!(offers.iter().any(|o| o == SHELL)),
        other => panic!("not the offers: {other:?}"),
    }
}

#[tokio::test]
async fn another_node_gets_a_report_with_status_and_version() {
    let desktop = desktop(None).await;
    let node = Node::start(Config {
        name: Some("build-box"),
        ..Config::default()
    })
    .await;
    node.join(&desktop.ticket(DeviceKind::Node).await).await;
    let other = peer_node(&desktop).await;
    let conn = other.reach(node.id).await;
    let mut stream = ready(&conn, REPORT).await;
    stream
        .tx
        .send(&ReportRequest {
            since: 0,
            max_bytes: 1 << 16,
        })
        .await
        .expect("the request is sent");
    let reply = timeout(LIMIT, stream.rx.recv::<ReportReply>())
        .await
        .expect("the report came in time")
        .expect("the report reads");
    let Some(ReportReply::Share {
        status,
        version,
        os,
        ..
    }) = reply
    else {
        panic!("not shared: {reply:?}");
    };
    assert_eq!(version, env!("CARGO_PKG_VERSION"));
    assert!(status.contains("build-box"), "{status}");
    assert!(!os.is_empty());
}

#[tokio::test]
async fn the_node_collects_from_its_mesh_for_the_cli() {
    let desktop = desktop(None).await;
    let mut a = Node::start(Config {
        name: Some("box-a"),
        ..Config::default()
    })
    .await;
    let b = Node::start(Config {
        name: Some("box-b"),
        ..Config::default()
    })
    .await;
    a.join(&desktop.ticket(DeviceKind::Node).await).await;
    b.join(&desktop.ticket(DeviceKind::Node).await).await;
    // Until `a` has its link to `b`, `b` is offline in the report.
    let shares = timeout(LIMIT, async {
        loop {
            let shares = beep_node::link::peers::collect_report(a.dir.path(), 0)
                .await
                .expect("the node collects")
                .expect("the node runs");
            if shares
                .iter()
                .any(|share| share.id == b.id && share.answer == Answer::Answered)
            {
                return shares;
            }
            // Not back to back: the next round waits for a's next log line, a second at most.
            if let Ok(Some(line)) = timeout(Duration::from_secs(1), a.lines.recv()).await {
                a.log.push(line);
            }
        }
    })
    .await
    .expect("b answered in time");
    let answered: Vec<_> = shares
        .iter()
        .filter(|share| share.answer == Answer::Answered)
        .map(|share| share.id)
        .collect();
    assert_eq!(answered.len(), 2, "{shares:?}");
    assert!(answered.contains(&a.id), "{shares:?}");
    let own = shares
        .iter()
        .find(|share| share.id == a.id)
        .expect("a's own share");
    assert!(own.status.contains("box-a"), "{}", own.status);
}

#[cfg(unix)]
#[tokio::test]
async fn a_silent_node_on_the_socket_does_not_stop_the_report() {
    let dir = TempDir::new().expect("a temp folder");
    let listener =
        tokio::net::UnixListener::bind(dir.path().join("mesh.sock")).expect("the socket binds");
    // Accepts and never writes, as a node whose workers are stuck.
    let _held = tokio::spawn(async move {
        let mut open = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            open.push(stream);
        }
    });
    let started = tokio::time::Instant::now();
    let collected =
        beep_node::link::peers::collect_report_within(dir.path(), 0, Duration::from_secs(1)).await;
    let silent = collected
        .expect_err("a silent node gives no report")
        .downcast::<ReportError>()
        .expect("the error says the node was silent");
    assert!(matches!(silent, ReportError::Silent(1)), "{silent:?}");
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn a_node_still_links_to_a_server_when_it_has_one() {
    let (hellos, mut seen) = mpsc::unbounded_channel();
    let desktop = desktop(Some(Server(hellos))).await;
    let mut node = Node::start(Config {
        server: Some(desktop.id()),
        ..Config::default()
    })
    .await;
    node.join(&desktop.ticket(DeviceKind::Node).await).await;
    // The node keeps dialing its server; once the mesh link taught its
    // endpoint where the server is, the next try gets through.
    let hello = timeout(LIMIT, seen.recv())
        .await
        .expect("the node linked to its server in time")
        .expect("the server saw the node");
    assert_eq!(hello.name, "box-a");
    assert!(hello.offers.iter().any(|o| o == SHELL));
    // And the mesh works beside it.
    desktop.reach(node.id).await;
    node.logged("connected as").await;
}

/// What a server of either age sees of a node's first message: the protocol
/// the connection settled on, and the hello's bytes as sent. It answers the
/// way that age does, in maps on `beep-node/1`.
#[derive(Debug)]
struct Raw(mpsc::UnboundedSender<(Vec<u8>, Vec<u8>)>);

impl ProtocolHandler for Raw {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let Ok((send, recv)) = connection.accept_bi().await else {
            return Ok(());
        };
        let mut frames = FramedRead::new(recv, LengthDelimitedCodec::new());
        if let Some(Ok(hello)) = frames.next().await {
            let mut out = FramedWrite::new(send, LengthDelimitedCodec::new());
            let welcome = Down::Welcome { tools: false };
            let body = if connection.alpn() == NODE_ALPN_NAMED {
                rmp_serde::to_vec_named(&welcome)
            } else {
                rmp_serde::to_vec(&welcome)
            };
            out.send(body.expect("a welcome encodes").into()).await.ok();
            self.0
                .send((connection.alpn().to_vec(), hello.to_vec()))
                .ok();
        }
        connection.closed().await;
        Ok(())
    }
}

/// A desktop that serves nodes only on `alpn`, and the first message each node sends.
async fn desktop_raw(alpn: &'static [u8]) -> (Member, mpsc::UnboundedReceiver<(Vec<u8>, Vec<u8>)>) {
    let (links, seen) = mpsc::unbounded_channel();
    let key = SecretKey::generate();
    let dir = TempDir::new().expect("a temp folder");
    let endpoint = bind(&key).await;
    let mesh = Mesh::create(dir.path(), endpoint.clone(), "desktop")
        .await
        .expect("the mesh is created");
    let (peer, join) = mesh.protocols();
    let router = Router::builder(endpoint)
        .accept(PEER_ALPN, peer)
        .accept(JOIN_ALPN, join)
        .accept(alpn, Raw(links))
        .spawn();
    let member = Member {
        mesh,
        _router: router,
        _dir: dir,
    };
    (member, seen)
}

async fn first_message_to(alpn: &'static [u8]) -> (Vec<u8>, Vec<u8>) {
    let (desktop, mut seen) = desktop_raw(alpn).await;
    let mut node = Node::start(Config {
        server: Some(desktop.id()),
        ..Config::default()
    })
    .await;
    node.join(&desktop.ticket(DeviceKind::Node).await).await;
    let first = timeout(LIMIT, seen.recv())
        .await
        .expect("the node linked to its server in time")
        .expect("the server saw the node");
    node.logged("connected as").await;
    first
}

#[tokio::test]
async fn a_node_talks_compact_to_a_server_that_serves_beep_node_2() {
    let (alpn, hello) = first_message_to(NODE_ALPN).await;
    assert_eq!(alpn, NODE_ALPN);
    // An array of the hello's nine fields.
    assert_eq!(hello[0], 0x99);
    beep_proto::lenient::from_slice::<NodeHello>(&hello).expect("the hello reads");
}

#[tokio::test]
async fn a_node_falls_back_to_maps_for_a_server_from_before_beep_node_2() {
    let (alpn, hello) = first_message_to(NODE_ALPN_NAMED).await;
    assert_eq!(alpn, NODE_ALPN_NAMED);
    // A map, and readable by the plain reader the released server has.
    assert_eq!(hello[0], 0x89);
    let hello: NodeHello = rmp_serde::from_slice(&hello).expect("the released server reads it");
    assert_eq!(hello.name, "box-a");
}

/// The job id a background `run_on` reports.
fn job_id(text: &str) -> String {
    let rest = text.split("Job id: ").nth(1).expect("a job id is named");
    rest.chars().take_while(char::is_ascii_digit).collect()
}

async fn run_background(conn: &Link, command: &str) -> Reply {
    tool(
        conn,
        Tool::RunOn,
        json!({ "machine": "box-a", "command": command, "background": true }),
    )
    .await
}

#[tokio::test]
async fn a_node_peer_cannot_fill_the_desktops_tasks_or_read_its_jobs() {
    let desktop = desktop(None).await;
    let node = Node::start(Config::default()).await;
    node.join(&desktop.ticket(DeviceKind::Node).await).await;
    let first = peer_node(&desktop).await;
    let second = peer_node(&desktop).await;
    let from_desktop = desktop.reach(node.id).await;
    let from_first = first.reach(node.id).await;
    let from_second = second.reach(node.id).await;
    // The desktop's own job, which a node peer must not see.
    let mut process = ready(&from_desktop, "process/1").await;
    process
        .tx
        .send(&ProcessRequest::Start {
            command: "sleep 30".to_owned(),
            cwd: None,
        })
        .await
        .expect("the start is sent");
    let Some(ProcessReply::Started { id: desktop_job }) = process
        .rx
        .recv::<ProcessReply>()
        .await
        .expect("a reply reads")
    else {
        panic!("the desktop's job did not start");
    };
    for tool_call in [Tool::JobStatus, Tool::Cancel] {
        let Reply::Done { failed, text } = tool(
            &from_first,
            tool_call,
            json!({ "id": desktop_job.to_string() }),
        )
        .await
        else {
            panic!("not done");
        };
        assert!(failed && text.contains("No background process"), "{text}");
    }
    // One node peer's job is not another's.
    let Reply::Done { text, failed } = run_background(&from_first, "sleep 30").await else {
        panic!("not done");
    };
    assert!(!failed, "{text}");
    let Reply::Done { failed, text } = tool(
        &from_second,
        Tool::JobStatus,
        json!({ "id": job_id(&text) }),
    )
    .await
    else {
        panic!("not done");
    };
    assert!(failed && text.contains("No background process"), "{text}");
    // Filling its own slots leaves the desktop's `process/1` working.
    let mut refused = false;
    for _ in 0..40 {
        let Reply::Done { failed, .. } = run_background(&from_first, "sleep 30").await else {
            panic!("not done");
        };
        refused |= failed;
    }
    assert!(refused, "the peer's slots never ran out");
    process
        .tx
        .send(&ProcessRequest::Start {
            command: "echo still-works".to_owned(),
            cwd: None,
        })
        .await
        .expect("the start is sent");
    assert!(matches!(
        process
            .rx
            .recv::<ProcessReply>()
            .await
            .expect("a reply reads"),
        Some(ProcessReply::Started { .. })
    ));
}

#[tokio::test]
async fn joining_with_a_search_folder_has_the_node_look_there() {
    let desktop = desktop(None).await;
    let node = Node::start(Config::default()).await;
    let projects = TempDir::new().expect("a temp folder");
    let search = projects.path().to_str().expect("a utf-8 path");
    node.join_with(
        &desktop.ticket(DeviceKind::Node).await,
        &["--search", search],
    )
    .await;
    let saved = beep_node::link::state::saved_search(node.dir.path()).expect("node.toml reads");
    assert_eq!(saved, beep_node::link::state::Search::parse(search));
}

#[tokio::test]
async fn a_search_folder_that_is_not_one_refuses_the_join() {
    let desktop = desktop(None).await;
    let node = Node::start(Config::default()).await;
    let ticket = desktop.ticket(DeviceKind::Node).await;
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_beep-node"))
        .args(["mesh", "join", "-", "--search", "/no/such/folder", "--dir"])
        .arg(node.dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("the command starts");
    drop(cmd.stdin.take());
    let out = timeout(LIMIT, cmd.wait_with_output())
        .await
        .expect("the command finished in time")
        .expect("the command ran");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--search"));
    // Nothing was spent: the same ticket still joins.
    node.join(&ticket).await;
}

#[cfg(unix)]
#[tokio::test]
async fn the_control_socket_is_private_and_the_ticket_stays_out_of_the_log() {
    use std::os::unix::fs::PermissionsExt;
    let desktop = desktop(None).await;
    let mut node = Node::start(Config::default()).await;
    let ticket = desktop.ticket(DeviceKind::Node).await;
    node.join(&ticket).await;
    desktop.reach(node.id).await;
    let mode = fs::metadata(node.dir.path().join("mesh.sock"))
        .expect("the socket exists")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    node.logged("joined a mesh").await;
    let text = ticket.encode_string();
    while let Ok(line) = node.lines.try_recv() {
        node.log.push(line);
    }
    assert!(node.log.iter().all(|line| !line.contains(&text)));
}

#[tokio::test]
async fn a_node_leaves_its_mesh_and_joins_another() {
    let first = desktop(None).await;
    let second = desktop(None).await;
    let node = Node::start(Config::default()).await;
    node.join(&first.ticket(DeviceKind::Node).await).await;
    first.reach(node.id).await;
    node.leave().await;
    node.join(&second.ticket(DeviceKind::Node).await).await;
    second.reach(node.id).await;
}

#[tokio::test]
async fn a_desktop_removed_mid_session_loses_its_stream() {
    let first = desktop(None).await;
    let node = Node::start(Config::default()).await;
    node.join(&first.ticket(DeviceKind::Node).await).await;
    // A second desktop joins through the first.
    let ticket = first.ticket(DeviceKind::Desktop).await;
    let dir = TempDir::new().expect("a temp folder");
    let endpoint = bind(&SecretKey::generate()).await;
    let mesh = Mesh::join(dir.path(), endpoint.clone(), &ticket, "second")
        .await
        .expect("the desktop joins");
    let second = Member {
        _router: router(endpoint, &mesh, None),
        mesh,
        _dir: dir,
    };
    let conn = second.reach(node.id).await;
    let mut shell = ready(&conn, SHELL).await;
    shell
        .tx
        .send(&ShellRequest::Run {
            command: "echo before".to_owned(),
            timeout_secs: 30,
        })
        .await
        .expect("the run is sent");
    while !matches!(
        shell.rx.recv::<ShellEvent>().await.expect("an event reads"),
        Some(ShellEvent::Done { .. })
    ) {}
    first
        .mesh
        .remove(second.id())
        .await
        .expect("the removal is signed");
    // The shell stream ends, and the shell with it.
    timeout(LIMIT, async {
        loop {
            let sent = shell
                .tx
                .send(&ShellRequest::Run {
                    command: "echo after".to_owned(),
                    timeout_secs: 30,
                })
                .await;
            match shell.rx.recv::<ShellEvent>().await {
                Ok(Some(_)) if sent.is_ok() => {}
                _ => return,
            }
        }
    })
    .await
    .expect("the removed desktop kept its stream");
}

/// A server that pushes what the test tells it to, over the control stream
/// and over streams it opens on the node.
#[derive(Debug)]
struct Pushing(mpsc::UnboundedSender<ServerLink>);

struct ServerLink {
    conn: Connection,
    tx: LinkTx,
}

impl ProtocolHandler for Pushing {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let Ok((send, recv)) = connection.accept_bi().await else {
            return Ok(());
        };
        let (mut tx, mut rx) = link(send, recv);
        if let Ok(Some(_)) = rx.recv::<NodeHello>().await {
            tx.send(&Down::Welcome { tools: false }).await.ok();
            let conn = connection.clone();
            self.0.send(ServerLink { conn, tx }).ok();
        }
        connection.closed().await;
        Ok(())
    }
}

async fn desktop_pushing() -> (Member, mpsc::UnboundedReceiver<ServerLink>) {
    let (links, seen) = mpsc::unbounded_channel();
    let key = SecretKey::generate();
    let dir = TempDir::new().expect("a temp folder");
    let endpoint = bind(&key).await;
    let mesh = Mesh::create(dir.path(), endpoint.clone(), "desktop")
        .await
        .expect("the mesh is created");
    let (peer, join) = mesh.protocols();
    let router = Router::builder(endpoint)
        .accept(PEER_ALPN, peer)
        .accept(JOIN_ALPN, join)
        .accept(NODE_ALPN, Pushing(links))
        .spawn();
    let member = Member {
        mesh,
        _router: router,
        _dir: dir,
    };
    (member, seen)
}

fn env_credentials(vars: &[(&str, &str)]) -> MachineCredentials {
    MachineCredentials {
        env: vars
            .iter()
            .map(|(name, value)| EnvSecret {
                name: (*name).to_owned(),
                value: SecretText::new(*value),
            })
            .collect(),
        forgejo: Vec::new(),
    }
}

async fn set_credential(from: &Member, id: &str, name: &str, value: &str) {
    let credential = Credential::Env(EnvSecret {
        name: name.to_owned(),
        value: SecretText::new(value),
    });
    from.mesh
        .set(
            Key::Credential(id.to_owned()),
            Some(credential.to_bytes().expect("a credential encodes")),
        )
        .await
        .expect("the credential is written");
}

/// What a command on the node sees of these env vars, as `got[a][b]`.
async fn env_seen(conn: &Link, names: [&str; 2]) -> String {
    let command = format!(
        "printf 'got[%s][%s]\\n' \"${}\" \"${}\"",
        names[0], names[1]
    );
    let Reply::Done { text, failed } = tool(
        conn,
        Tool::RunOn,
        json!({ "machine": "box-a", "command": command }),
    )
    .await
    else {
        panic!("not done");
    };
    assert!(!failed, "{text}");
    text.lines()
        .find(|line| line.starts_with("got["))
        .unwrap_or_else(|| panic!("no output in {text:?}"))
        .to_owned()
}

async fn sync_call(conn: &Link, request: SyncRequest) -> SyncReply {
    let mut stream = ready(conn, SYNC).await;
    stream.tx.send(&request).await.expect("the request is sent");
    timeout(LIMIT, stream.rx.recv::<SyncReply>())
        .await
        .expect("sync answered in time")
        .expect("the reply reads")
        .expect("sync replied")
}

async fn sync_names(conn: &Link) -> Vec<String> {
    let SyncReply::Status { projects } = sync_call(conn, SyncRequest::Status {}).await else {
        panic!("not a status");
    };
    let mut names: Vec<String> = projects.into_iter().map(|p| p.name).collect();
    names.sort();
    names
}

/// The reason a start was turned down, or `None` when the session started.
async fn start_refusal(conn: &Link, cwd: &Path) -> Option<String> {
    let mut agent = ready(conn, "codex/1").await;
    agent
        .tx
        .send(&AgentRequest::Start {
            cwd: Some(cwd.display().to_string()),
            prompt: "go".to_owned(),
            model: None,
            effort: None,
            checkout: None,
        })
        .await
        .expect("the start is sent");
    timeout(LIMIT, async {
        loop {
            match agent.rx.recv::<AgentEvent>().await {
                Ok(Some(AgentEvent::Attached { .. })) => return None,
                Ok(Some(AgentEvent::Ended { reason })) => return Some(reason),
                Ok(Some(_)) => {}
                Ok(None) => return Some("the stream closed".to_owned()),
                Err(e) => return Some(format!("{e:#}")),
            }
        }
    })
    .await
    .expect("the start answered in time")
}

fn adapter(log: &Path) -> (String, Vec<String>) {
    adapter_of(ADAPTER, log)
}

/// What the server pushes: credentials, agent settings and a sync list, each
/// of which a node out of a mesh would take. Returns once the node logged
/// that it ignored all three.
async fn push_everything(
    server: &mut ServerLink,
    node: &mut Node,
    shared: &Path,
    vars: &[(&str, &str)],
) {
    server
        .tx
        .send(&Down::Credentials(env_credentials(vars)))
        .await
        .expect("the credentials are sent");
    server
        .tx
        .send(&Down::AgentSettings(AgentSettings {
            harnesses: vec![HarnessConfig::new("codex")],
            flsh: None,
        }))
        .await
        .expect("the settings are sent");
    let (send, recv) = server.conn.open_bi().await.expect("a stream opens");
    let (mut tx, mut rx) = link(send, recv);
    tx.send(&Open {
        protocol: SYNC.to_owned(),
    })
    .await
    .expect("the open is sent");
    assert!(matches!(
        rx.recv::<Opened>().await.expect("an answer reads"),
        Some(Opened::Ready {})
    ));
    tx.send(&SyncRequest::Configure {
        projects: vec![SyncProject {
            name: "server-proj".to_owned(),
            folder: shared.join("server-proj").display().to_string(),
            peers: Vec::new(),
            paused: false,
        }],
    })
    .await
    .expect("the list is sent");
    timeout(LIMIT, rx.recv::<SyncReply>())
        .await
        .expect("sync answered in time")
        .expect("the reply reads");
    node.logged("ignored credentials from the server").await;
    node.logged("ignored agent settings from the server").await;
    node.logged("ignored the server's sync list").await;
}

#[cfg(unix)]
#[tokio::test]
async fn the_servers_sync_and_credentials_do_not_overwrite_the_mesh() {
    let (desktop, mut links) = desktop_pushing().await;
    let shared = TempDir::new().expect("a temp folder");
    let work = TempDir::new().expect("a temp folder");
    let log = work.path().join("sent.jsonl");
    let mut node = Node::start(Config {
        server: Some(desktop.id()),
        agent: Some(adapter(&log)),
        files: vec![shared.path().to_owned()],
        ..Config::default()
    })
    .await;
    // The node finds its server through the mesh link, so it joins first.
    node.join(&desktop.ticket(DeviceKind::Node).await).await;
    let mut server = timeout(LIMIT, links.recv())
        .await
        .expect("the node linked to its server in time")
        .expect("the server saw the node");
    set_credential(&desktop, "gh", "MESH_VAR", "from-the-mesh").await;
    let harness = HarnessConfig {
        enabled: false,
        ..HarnessConfig::new("codex")
    };
    desktop
        .mesh
        .set(
            Key::Harness("codex".to_owned()),
            Some(harness.to_bytes().expect("a harness encodes")),
        )
        .await
        .expect("the setting is written");
    let folder = shared.path().join("mesh-proj");
    fs::create_dir(&folder).expect("the project folder");
    let spec = SyncSpec {
        folders: [(node.id, folder.display().to_string())].into(),
        paused: false,
    };
    desktop
        .mesh
        .set(
            Key::Sync("mesh-proj".to_owned()),
            Some(spec.to_bytes().expect("a spec encodes")),
        )
        .await
        .expect("the spec is written");
    node.logged("from the mesh: 1 credentials").await;
    node.logged("from the mesh: sync mesh-proj").await;
    let conn = desktop.reach(node.id).await;
    assert_eq!(
        env_seen(&conn, ["MESH_VAR", "SRV_LATER"]).await,
        "got[from-the-mesh][]"
    );
    assert_eq!(sync_names(&conn).await, ["mesh-proj"]);
    assert!(start_refusal(&conn, work.path()).await.is_some());
    // Whatever the server pushes now, the mesh has spoken.
    push_everything(
        &mut server,
        &mut node,
        shared.path(),
        &[
            ("SRV_LATER", "from-the-server"),
            ("MESH_VAR", "server-override"),
        ],
    )
    .await;
    assert_eq!(
        env_seen(&conn, ["MESH_VAR", "SRV_LATER"]).await,
        "got[from-the-mesh][]"
    );
    assert_eq!(sync_names(&conn).await, ["mesh-proj"]);
    // The agent is turned off by the mesh, not by anything else: when the
    // mesh drops the setting, it starts.
    assert!(start_refusal(&conn, work.path()).await.is_some());
    desktop
        .mesh
        .set(Key::Harness("codex".to_owned()), None)
        .await
        .expect("the setting is deleted");
    node.logged("from the mesh: settings for 0 agents").await;
    assert_eq!(start_refusal(&conn, work.path()).await, None);
}

/// Two nodes of one mesh that keep the project `proj` in step, each with
/// the folder under a root it shares.
struct Pair {
    desktop: Member,
    a: Node,
    b: Node,
    a_folder: PathBuf,
    b_folder: PathBuf,
    roots: [TempDir; 2],
    /// Where the fake agent on `a` logs what it was sent.
    work: TempDir,
}

async fn synced_pair() -> Pair {
    synced_pair_with_agent(false).await
}

/// `a` runs the fake coding agent when `agent` is set.
async fn synced_pair_with_agent(agent: bool) -> Pair {
    synced_pair_with(agent.then_some(ADAPTER), None).await
}

/// Waits for `path` to hold `text`, woken by the folder's file events; the
/// outer limit is the only clock.
async fn has_text(path: &Path, text: &str) {
    use notify::{RecursiveMode, Watcher};
    let (send, mut events) = mpsc::unbounded_channel();
    let mut watcher = notify::recommended_watcher(move |_| {
        send.send(()).ok();
    })
    .expect("a watcher starts");
    watcher
        .watch(
            path.parent().expect("a folder"),
            RecursiveMode::NonRecursive,
        )
        .expect("the folder is watched");
    timeout(LIMIT, async {
        // Checked after the watch starts, so no change falls between.
        while !fs::read_to_string(path).is_ok_and(|have| have == text) {
            events.recv().await.expect("the watcher lives");
        }
    })
    .await
    .unwrap_or_else(|_| panic!("{} never held {text:?}", path.display()));
}

#[tokio::test]
async fn a_project_syncs_between_two_nodes_with_no_server() {
    let pair = synced_pair().await;
    fs::write(pair.a_folder.join("from-a.txt"), "made on a").expect("a file");
    has_text(&pair.b_folder.join("from-a.txt"), "made on a").await;
    fs::write(pair.b_folder.join("from-b.txt"), "made on b").expect("a file");
    has_text(&pair.a_folder.join("from-b.txt"), "made on b").await;
}

/// Two nodes that share no folders, each with a project folder of its own,
/// and what the desktop writes to sync them.
struct Bare {
    desktop: Member,
    a: Node,
    b: Node,
    a_folder: PathBuf,
    b_folder: PathBuf,
    _roots: [TempDir; 2],
}

/// The node `a` is started by the caller, who can set its Syncthing up first.
async fn bare_nodes(a: Node, a_root: TempDir, a_folder: PathBuf) -> Bare {
    let desktop = desktop(None).await;
    let b_root = TempDir::new().expect("a temp folder");
    let b_folder = b_root.path().join("proj");
    fs::create_dir(&b_folder).expect("a project folder");
    let b = Node::start(Config {
        name: Some("box-b"),
        ..Config::default()
    })
    .await;
    a.join(&desktop.ticket(DeviceKind::Node).await).await;
    b.join(&desktop.ticket(DeviceKind::Node).await).await;
    Bare {
        desktop,
        a,
        b,
        a_folder,
        b_folder,
        _roots: [a_root, b_root],
    }
}

async fn bare_node() -> (Node, TempDir, PathBuf) {
    let root = TempDir::new().expect("a temp folder");
    let folder = root.path().join("proj");
    fs::create_dir(&folder).expect("a project folder");
    let node = Node::start(Config {
        name: Some("box-a"),
        ..Config::default()
    })
    .await;
    (node, root, folder)
}

impl Bare {
    /// Writes the record, and waits for each node to take it.
    async fn sync(&mut self) {
        let spec = SyncSpec {
            folders: [
                (self.a.id, self.a_folder.display().to_string()),
                (self.b.id, self.b_folder.display().to_string()),
            ]
            .into(),
            paused: false,
        };
        self.desktop
            .mesh
            .set(
                Key::Sync("proj".to_owned()),
                Some(spec.to_bytes().expect("a spec encodes")),
            )
            .await
            .expect("the spec is written");
        self.a.logged("from the mesh: sync proj").await;
        self.b.logged("from the mesh: sync proj").await;
    }
}

/// What `conn`'s node says of the project.
async fn proj_status(conn: &Link) -> Option<beep_proto::node::sync::ProjectStatus> {
    let SyncReply::Status { projects } = sync_call(conn, SyncRequest::Status {}).await else {
        panic!("not a status");
    };
    projects.into_iter().find(|p| p.name == "proj")
}

/// Asks `conn`'s node until `done` holds of the project. A sync status has
/// nothing to wait on from outside; the outer limit is the only clock.
async fn until_status(conn: &Link, done: impl Fn(&beep_proto::node::sync::ProjectStatus) -> bool) {
    timeout(LIMIT, async {
        while !proj_status(conn).await.as_ref().is_some_and(&done) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the status came in time");
}

#[tokio::test]
async fn a_project_syncs_on_nodes_that_share_no_folders_and_after_a_restart() {
    let (a, root, folder) = bare_node().await;
    let mut pair = bare_nodes(a, root, folder).await;
    pair.sync().await;
    fs::write(pair.a_folder.join("from-a.txt"), "made on a").expect("a file");
    has_text(&pair.b_folder.join("from-a.txt"), "made on a").await;
    fs::write(pair.b_folder.join("from-b.txt"), "made on b").expect("a file");
    has_text(&pair.a_folder.join("from-b.txt"), "made on b").await;

    // The saved list keeps to `files`; the mesh names the project again.
    pair.a = pair.a.restart().await;
    pair.a.logged("from the mesh: sync proj").await;
    fs::write(pair.b_folder.join("after.txt"), "after a restart").expect("a file");
    has_text(&pair.a_folder.join("after.txt"), "after a restart").await;
}

#[tokio::test]
async fn a_node_the_record_drops_stops_syncing_and_keeps_its_files() {
    let (a, root, folder) = bare_node().await;
    let mut pair = bare_nodes(a, root, folder).await;
    pair.sync().await;
    fs::write(pair.a_folder.join("from-a.txt"), "made on a").expect("a file");
    has_text(&pair.b_folder.join("from-a.txt"), "made on a").await;
    fs::write(pair.b_folder.join("from-b.txt"), "made on b").expect("a file");
    has_text(&pair.a_folder.join("from-b.txt"), "made on b").await;

    // The project is taken off b: one machine is left, so the record goes.
    pair.desktop
        .mesh
        .set(Key::Sync("proj".to_owned()), None)
        .await
        .expect("the record goes");
    let conn_b = pair.desktop.reach(pair.b.id).await;
    timeout(LIMIT, async {
        while proj_status(&conn_b).await.is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("b stopped syncing in time");

    // What a does now never reaches b, and b keeps every file it had.
    fs::write(pair.a_folder.join("later.txt"), "after the removal").expect("a file");
    fs::remove_file(pair.a_folder.join("from-b.txt")).expect("a file goes");
    let conn_a = pair.desktop.reach(pair.a.id).await;
    timeout(LIMIT, async {
        while proj_status(&conn_a).await.is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("a stopped syncing in time");
    assert_eq!(
        fs::read_to_string(pair.b_folder.join("from-a.txt")).expect("kept"),
        "made on a"
    );
    assert_eq!(
        fs::read_to_string(pair.b_folder.join("from-b.txt")).expect("kept"),
        "made on b"
    );
    assert!(!pair.b_folder.join("later.txt").exists());
}

#[tokio::test]
async fn checkouts_at_other_commits_merge_when_the_mesh_names_them() {
    let (a, root, folder) = bare_node().await;
    let mut pair = bare_nodes(a, root, folder).await;
    for (folder, file) in [(&pair.a_folder, "a.txt"), (&pair.b_folder, "b.txt")] {
        git_in(folder, &["init", "-q", "-b", "main"]);
        fs::write(folder.join("same.txt"), file).expect("a file");
        fs::write(folder.join(file), "only here").expect("a file");
        git_in(folder, &["add", "."]);
        git_in(folder, &["commit", "-q", "-m", file]);
        // Not committed, as on a machine where work is under way.
        fs::write(folder.join("same.txt"), format!("edited beside {file}")).expect("a file");
    }
    pair.sync().await;

    for (folder, theirs) in [(&pair.a_folder, "b.txt"), (&pair.b_folder, "a.txt")] {
        has_text(&folder.join(theirs), "only here").await;
    }
    // Neither side's edit is lost: one is the file, the other a conflict copy.
    timeout(LIMIT, async {
        loop {
            let holds = |folder: &Path, text: &str| {
                fs::read_dir(folder)
                    .expect("the folder reads")
                    .flatten()
                    .any(|file| fs::read_to_string(file.path()).is_ok_and(|read| read == text))
            };
            if [&pair.a_folder, &pair.b_folder].iter().all(|folder| {
                holds(folder, "edited beside a.txt") && holds(folder, "edited beside b.txt")
            }) {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("both edits are kept on both machines");
}

#[tokio::test]
async fn a_new_empty_folder_takes_the_whole_checkout_with_its_git() {
    let (a, root, folder) = bare_node().await;
    let mut pair = bare_nodes(a, root, folder).await;
    let (from, to) = (pair.a_folder.clone(), pair.b_folder.clone());
    git_in(&from, &["init", "-q", "-b", "main"]);
    git_in(
        &from,
        &[
            "remote",
            "add",
            "origin",
            "https://example.com/acme/widget.git",
        ],
    );
    fs::create_dir(from.join("src")).expect("a folder");
    fs::write(from.join("src/main.rs"), "fn main() {}\n").expect("a file");
    fs::write(from.join(".gitignore"), "*.env\n").expect("a file");
    git_in(&from, &["add", "."]);
    git_in(&from, &["commit", "-q", "-m", "first"]);
    git_in(&from, &["branch", "side"]);
    fs::write(from.join("src/lib.rs"), "pub fn f() {}\n").expect("a file");
    git_in(&from, &["add", "."]);
    git_in(&from, &["commit", "-q", "-m", "second"]);
    fs::write(from.join("local.env"), "ignored").expect("a file");
    fs::write(from.join("src/main.rs"), "fn main() { todo!() }\n").expect("an edit");
    let head = git_in(&from, &["rev-parse", "HEAD"]);
    let log = git_in(&from, &["log", "--format=%H"]);

    pair.sync().await;

    let conn_b = pair.desktop.reach(pair.b.id).await;
    timeout(LIMIT, async {
        while !git_is_at(&to, &head) || git_in(&to, &["status", "--porcelain"]) != "M src/main.rs" {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the branch and index came in time");

    assert_eq!(git_in(&to, &["log", "--format=%H"]), log);
    assert_eq!(
        git_in(&to, &["rev-parse", "side"]),
        git_in(&from, &["rev-parse", "side"])
    );
    assert_eq!(
        git_in(&to, &["remote", "get-url", "origin"]),
        "https://example.com/acme/widget.git"
    );
    assert!(git_in(&to, &["for-each-ref", "refs/beep-conflicts"]).is_empty());
    has_text(&to.join("src/main.rs"), "fn main() { todo!() }\n").await;
    has_text(&to.join("local.env"), "ignored").await;
    assert_eq!(
        fs::read_to_string(from.join("src/main.rs")).expect("the edit stays"),
        "fn main() { todo!() }\n"
    );
    let conn_a = pair.desktop.reach(pair.a.id).await;
    until_status(&conn_b, |status| {
        status.state == beep_proto::node::sync::State::Idle && status.pending == 0
    })
    .await;
    let source = proj_status(&conn_a).await.expect("a status");
    let target = proj_status(&conn_b).await.expect("a status");
    assert_eq!(target.files, source.files);
}

/// What a fake Syncthing says of its one folder: paused or not, and the
/// number of its last event.
#[derive(Clone, Copy)]
struct FakeFolder {
    paused: bool,
    event: u64,
}

fn set_paused(folder: &watch::Sender<FakeFolder>, paused: bool) {
    folder.send_modify(|folder| {
        folder.paused = paused;
        folder.event += 1;
    });
}

/// A fake Syncthing GUI listing one folder at `path`, with `/rest/events` held
/// open until the folder changes, and the `config.xml` that points `node` at it.
async fn fake_syncthing(
    node: &Node,
    path: &Path,
    folder: watch::Sender<FakeFolder>,
) -> AbortOnDropHandle<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
    let addr = listener.local_addr().expect("an address");
    let config = format!(
        r#"<configuration><gui tls="false"><address>{addr}</address><apikey>fake-key</apikey></gui></configuration>"#
    );
    for home in ["state/syncthing", "Library/Application Support/Syncthing"] {
        let home = node.home.path().join(home);
        fs::create_dir_all(&home).expect("a config folder");
        fs::write(home.join("config.xml"), &config).expect("a config");
    }
    let path = json!(path).to_string();
    let task = tokio::spawn(async move {
        let mut answering = JoinSet::new();
        while let Ok((stream, _)) = listener.accept().await {
            answering.spawn(fake_answer(stream, folder.subscribe(), path.clone()));
        }
    });
    AbortOnDropHandle::new(task)
}

async fn fake_answer(mut stream: TcpStream, mut folder: watch::Receiver<FakeFolder>, path: String) {
    let mut request = Vec::new();
    let mut buf = [0; 1024];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => request.extend_from_slice(&buf[..n]),
        }
    }
    let text = String::from_utf8_lossy(&request);
    let line = text.lines().next().unwrap_or_default();
    let body = if let Some((_, query)) = line.split_once("since=") {
        let since: u64 = query
            .split('&')
            .next()
            .and_then(|n| n.parse().ok())
            .expect("a number");
        let event = folder
            .wait_for(|folder| folder.event > since)
            .await
            .expect("the test holds the folder")
            .event;
        format!(r#"[{{"id":{event},"type":"FolderResumed"}}]"#)
    } else {
        let paused = folder.borrow().paused;
        format!(r#"[{{"id":"game-1","label":"Game","path":{path},"paused":{paused}}}]"#)
    };
    let reply = format!(
        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(reply.as_bytes())
        .await
        .expect("the reply is sent");
}

#[tokio::test]
async fn a_folder_syncthing_holds_on_one_machine_syncs_only_while_syncthing_lets_go() {
    let (a, root, a_folder) = bare_node().await;
    let (folder, _) = watch::channel(FakeFolder {
        paused: false,
        event: 0,
    });
    let _api = fake_syncthing(&a, &a_folder, folder.clone()).await;
    // Started again to find the config, as a node started after Syncthing.
    let a = a.restart().await;
    let mut pair = bare_nodes(a, root, a_folder).await;
    pair.sync().await;
    let (conn_a, conn_b) = (
        pair.desktop.reach(pair.a.id).await,
        pair.desktop.reach(pair.b.id).await,
    );

    // Held, not refused: it is on the list, and the other side's files stay out.
    let held = |status: &beep_proto::node::sync::ProjectStatus| {
        status.error.as_deref() == Some("Already synced by Syncthing (Game)")
    };
    until_status(&conn_a, held).await;
    fs::write(pair.b_folder.join("early.txt"), "from b").expect("a file");
    until_status(&conn_b, |status| status.files >= 1).await;
    fs::write(pair.a_folder.join("syncthings.txt"), "stays").expect("a file");
    assert!(!pair.a_folder.join("early.txt").exists());
    assert!(!pair.b_folder.join("syncthings.txt").exists());

    // Paused in Syncthing: Beep's sync starts by itself, both ways.
    set_paused(&folder, true);
    pair.a.logged("Syncthing let go of it or paused it").await;
    has_text(&pair.a_folder.join("early.txt"), "from b").await;
    has_text(&pair.b_folder.join("syncthings.txt"), "stays").await;

    // Resumed: it stops at once, and what the other side writes stays out.
    set_paused(&folder, false);
    pair.a.logged("stopped syncing proj").await;
    let before = proj_status(&conn_b).await.expect("b syncs it").files;
    fs::write(pair.b_folder.join("late.txt"), "from b").expect("a file");
    until_status(&conn_b, |status| status.files > before).await;
    until_status(&conn_a, held).await;
    assert!(!pair.a_folder.join("late.txt").exists());
}

#[tokio::test]
async fn a_removed_machine_stops_syncing_at_once() {
    let mut pair = synced_pair().await;
    fs::write(pair.a_folder.join("before.txt"), "before").expect("a file");
    has_text(&pair.b_folder.join("before.txt"), "before").await;
    pair.desktop
        .mesh
        .remove(pair.b.id)
        .await
        .expect("the removal is signed");
    pair.a.logged("from the mesh: sync proj with 0 peers").await;
    fs::write(pair.a_folder.join("after.txt"), "after").expect("a file");
    // The removed machine keeps asking; the node refuses it.
    pair.a.logged("refused a sync connection").await;
    assert!(!pair.b_folder.join("after.txt").exists());
}

/// The Beep MCP server as a coding agent on the node has it: the command
/// the node handed the agent, run with the env the node gave it.
struct Mcp {
    _child: Child,
    stdin: ChildStdin,
    out: Lines<BufReader<ChildStdout>>,
    next: u64,
}

impl Mcp {
    /// Runs the server entry in the agent's `session/new`, the second line
    /// the fake adapter logged.
    async fn start(sent: &Path) -> Self {
        let text = fs::read_to_string(sent).expect("the adapter logged what it got");
        let new: serde_json::Value =
            serde_json::from_str(text.lines().nth(1).expect("a session/new")).expect("JSON");
        let server = &new["params"]["mcpServers"][0];
        let mut command = Command::new(server["command"].as_str().expect("a command"));
        for arg in server["args"].as_array().expect("arguments") {
            command.arg(arg.as_str().expect("an argument"));
        }
        for var in server["env"].as_array().expect("env vars") {
            command.env(
                var["name"].as_str().expect("a name"),
                var["value"].as_str().expect("a value"),
            );
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .expect("the MCP server starts");
        let stdin = child.stdin.take().expect("stdin is piped");
        let out = BufReader::new(child.stdout.take().expect("stdout is piped")).lines();
        let mut mcp = Self {
            _child: child,
            stdin,
            out,
            next: 0,
        };
        mcp.request("initialize", json!({ "protocolVersion": "2025-06-18" }))
            .await;
        mcp
    }

    async fn request(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        self.next += 1;
        let line = json!({ "jsonrpc": "2.0", "id": self.next, "method": method, "params": params });
        self.stdin
            .write_all(format!("{line}\n").as_bytes())
            .await
            .expect("the request is written");
        let answer = timeout(LIMIT, self.out.next_line())
            .await
            .expect("the MCP server answered in time")
            .expect("a line reads")
            .expect("the MCP server answered");
        serde_json::from_str(&answer).expect("the answer is JSON")
    }

    async fn tools(&mut self) -> Vec<String> {
        let answer = self.request("tools/list", json!({})).await;
        answer["result"]["tools"]
            .as_array()
            .expect("a tool list")
            .iter()
            .map(|tool| tool["name"].as_str().expect("a name").to_owned())
            .collect()
    }

    /// The text of the call and whether it failed.
    async fn call(&mut self, name: &str, args: serde_json::Value) -> (String, bool) {
        let answer = self
            .request("tools/call", json!({ "name": name, "arguments": args }))
            .await;
        let result = &answer["result"];
        (
            result["content"][0]["text"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            result["isError"].as_bool().unwrap_or(true),
        )
    }
}

/// Starts an agent session on the node, over the mesh, and waits for its
/// first turn; the MCP server of that session after it.
async fn agent_mcp(desktop: &Member, node: &Node, work: &Path, sent: &Path) -> Mcp {
    let conn = desktop.reach(node.id).await;
    assert_eq!(start_refusal(&conn, work).await, None);
    Mcp::start(sent).await
}

#[cfg(unix)]
#[tokio::test]
async fn an_agent_sends_files_between_machines_that_share_no_folders() {
    let desktop = desktop(None).await;
    let work = TempDir::new().expect("a temp folder");
    let sent = work.path().join("sent.jsonl");
    let a = Node::start(Config {
        name: Some("box-a"),
        agent: Some(adapter(&sent)),
        ..Config::default()
    })
    .await;
    let b = Node::start(Config {
        name: Some("box-b"),
        ..Config::default()
    })
    .await;
    a.join(&desktop.ticket(DeviceKind::Node).await).await;
    b.join(&desktop.ticket(DeviceKind::Node).await).await;
    desktop.reach(b.id).await;
    let mut mcp = agent_mcp(&desktop, &a, work.path(), &sent).await;
    timeout(LIMIT, async {
        while !mcp
            .call("list_machines", json!({}))
            .await
            .0
            .contains("box-b: online")
        {}
    })
    .await
    .expect("box-b came online in time");
    let note = work.path().join("note.txt");
    fs::write(&note, "for b").expect("a file");
    let dest = work.path().join("landed");
    let (text, failed) = mcp
        .call(
            "send_files",
            json!({
                "machine": "box-b",
                "paths": [note.display().to_string()],
                "dest": dest.display().to_string(),
            }),
        )
        .await;
    assert!(!failed, "{text}");
    assert_eq!(
        fs::read_to_string(dest.join("note.txt")).expect("the file landed"),
        "for b"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn an_agent_runs_a_command_on_another_machine_with_no_server() {
    let desktop = desktop(None).await;
    let work = TempDir::new().expect("a temp folder");
    let sent = work.path().join("sent.jsonl");
    let a = Node::start(Config {
        name: Some("box-a"),
        agent: Some(adapter(&sent)),
        ..Config::default()
    })
    .await;
    let b = Node::start(Config {
        name: Some("box-b"),
        ..Config::default()
    })
    .await;
    a.join(&desktop.ticket(DeviceKind::Node).await).await;
    b.join(&desktop.ticket(DeviceKind::Node).await).await;
    desktop.reach(b.id).await;
    let mut mcp = agent_mcp(&desktop, &a, work.path(), &sent).await;
    // The link to the other node may still be coming up.
    let listed = timeout(LIMIT, async {
        loop {
            let (listed, failed) = mcp.call("list_machines", json!({})).await;
            assert!(!failed, "{listed}");
            // Each machine says what it is, from its own status report, which comes after the link.
            let os_lines = listed
                .lines()
                .filter(|line| line.contains(std::env::consts::OS))
                .count();
            if listed.contains("box-b: online") && os_lines >= 2 {
                return listed;
            }
        }
    })
    .await
    .expect("box-b came online and reported in time");
    assert!(
        listed.contains("box-a") && listed.contains("box-b"),
        "{listed}"
    );
    let (text, failed) = mcp
        .call(
            "run_on",
            json!({ "machine": "box-b", "command": "printenv HOME" }),
        )
        .await;
    assert!(!failed, "{text}");
    let there = b.home.path().display().to_string();
    assert!(text.contains(&there), "{text}");
    // The job it starts there is found again by the id the call gave.
    let (text, failed) = mcp
        .call(
            "run_on",
            json!({ "machine": "box-b", "command": "sleep 300", "background": true }),
        )
        .await;
    assert!(!failed, "{text}");
    let id = text
        .split("Job id: ")
        .nth(1)
        .and_then(|rest| rest.split(". ").next())
        .expect("a job id")
        .to_owned();
    let (text, failed) = mcp.call("cancel", json!({ "id": id })).await;
    assert!(!failed, "{text}");
}

#[cfg(unix)]
#[tokio::test]
async fn memory_tools_are_absent_with_no_server() {
    let desktop = desktop(None).await;
    let work = TempDir::new().expect("a temp folder");
    let sent = work.path().join("sent.jsonl");
    let node = Node::start(Config {
        agent: Some(adapter(&sent)),
        ..Config::default()
    })
    .await;
    node.join(&desktop.ticket(DeviceKind::Node).await).await;
    let mut mcp = agent_mcp(&desktop, &node, work.path(), &sent).await;
    let tools = mcp.tools().await;
    assert!(tools.iter().any(|tool| tool == "run_on"), "{tools:?}");
    for memory in ["remember", "recall", "update_memory", "forget"] {
        assert!(!tools.iter().any(|tool| tool == memory), "{tools:?}");
    }
    let (text, failed) = mcp.call("remember", json!({ "content": "x" })).await;
    assert!(failed, "{text}");
}

/// Files under `dir` whose bytes hold `needle`.
fn files_holding(dir: &Path, needle: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_owned()];
    while let Some(folder) = pending.pop() {
        for entry in fs::read_dir(&folder).into_iter().flatten().flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => pending.push(path),
                Ok(kind) if kind.is_file() => {
                    let bytes = fs::read(&path).unwrap_or_default();
                    if bytes.windows(needle.len()).any(|w| w == needle.as_bytes()) {
                        found.push(path.strip_prefix(dir).expect("under dir").to_owned());
                    }
                }
                _ => {}
            }
        }
    }
    found
}

#[cfg(unix)]
#[tokio::test]
async fn a_deleted_credential_disappears_from_every_node_and_one_copy_is_kept() {
    use std::os::unix::fs::PermissionsExt;
    let desktop = desktop(None).await;
    let mut a = Node::start(Config::default()).await;
    let mut b = Node::start(Config {
        name: Some("box-b"),
        ..Config::default()
    })
    .await;
    a.join(&desktop.ticket(DeviceKind::Node).await).await;
    b.join(&desktop.ticket(DeviceKind::Node).await).await;
    let (from_a, from_b) = (desktop.reach(a.id).await, desktop.reach(b.id).await);
    set_credential(&desktop, "gh", "MESH_VAR", "kept-in-one-place").await;
    a.logged("from the mesh: 1 credentials").await;
    b.logged("from the mesh: 1 credentials").await;
    for conn in [&from_a, &from_b] {
        assert_eq!(
            env_seen(conn, ["MESH_VAR", "NOT_SET"]).await,
            "got[kept-in-one-place][]"
        );
    }
    // The mesh's state file is the only copy on disk, and it is private.
    for node in [&a, &b] {
        assert_eq!(
            files_holding(node.dir.path(), "kept-in-one-place"),
            [PathBuf::from("mesh2/state.log")]
        );
        let mode = fs::metadata(node.dir.path().join("mesh2/state.log"))
            .expect("the state file exists")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    // No log line holds it either.
    for node in [&a, &b] {
        assert!(
            node.log
                .iter()
                .all(|line| !line.contains("kept-in-one-place"))
        );
    }
    desktop
        .mesh
        .set(Key::Credential("gh".to_owned()), None)
        .await
        .expect("the credential is deleted");
    a.logged("from the mesh: 0 credentials").await;
    b.logged("from the mesh: 0 credentials").await;
    for conn in [&from_a, &from_b] {
        assert_eq!(env_seen(conn, ["MESH_VAR", "NOT_SET"]).await, "got[][]");
    }
}

fn free_port() -> u16 {
    std::net::UdpSocket::bind("127.0.0.1:0")
        .expect("a free port")
        .local_addr()
        .expect("its address")
        .port()
}

/// What a command on the restarted node sees of `MESH_VAR`, as `got[..]`.
/// The desktop may hold the old process's link for a moment; this asks
/// again on the new one.
async fn env_after_restart(desktop: &Member, node: &Node) -> String {
    let mut updates = desktop.mesh.updates();
    timeout(LIMIT, async {
        loop {
            updates.borrow_and_update();
            let conn = desktop.reach(node.id).await;
            let call = try_tool(
                &conn,
                Tool::RunOn,
                json!({ "command": "printf 'got[%s]\\n' \"$MESH_VAR\"" }),
            )
            .await;
            if let Ok(Reply::Done { text, .. }) = call {
                return text;
            }
            updates.changed().await.expect("the mesh is alive");
        }
    })
    .await
    .expect("the restarted node answered in time")
}

#[cfg(unix)]
#[tokio::test]
async fn a_deleted_credential_stays_gone_after_a_restart() {
    let desktop = desktop(None).await;
    let mut node = Node::start(Config {
        port: Some(free_port()),
        ..Config::default()
    })
    .await;
    node.join(&desktop.ticket(DeviceKind::Node).await).await;
    set_credential(&desktop, "gh", "MESH_VAR", "short-lived").await;
    node.logged("from the mesh: 1 credentials").await;
    desktop
        .mesh
        .set(Key::Credential("gh".to_owned()), None)
        .await
        .expect("the credential is deleted");
    node.logged("from the mesh: 0 credentials").await;
    let mut node = node.restart().await;
    // The deleted key is still held, so the family is the mesh's and empty.
    let applied = "from the mesh: 0 credentials";
    if !node.log.iter().any(|line| line.contains(applied)) {
        node.logged(applied).await;
    }
    let seen = env_after_restart(&desktop, &node).await;
    assert!(seen.contains("got[]"), "{seen}");
}

#[cfg(unix)]
#[tokio::test]
async fn credentials_come_back_after_a_restart_with_no_server() {
    let desktop = desktop(None).await;
    // The same port after the restart, so the desktop finds the node again
    // with no discovery.
    let mut node = Node::start(Config {
        port: Some(free_port()),
        ..Config::default()
    })
    .await;
    node.join(&desktop.ticket(DeviceKind::Node).await).await;
    set_credential(&desktop, "gh", "MESH_VAR", "still-here").await;
    node.logged("from the mesh: 1 credentials").await;
    let mut node = node.restart().await;
    // Nothing reaches it from a desktop: the state file is what it has. The
    // line may have come before the node was ready.
    let applied = "from the mesh: 1 credentials";
    if !node.log.iter().any(|line| line.contains(applied)) {
        node.logged(applied).await;
    }
    let seen = env_after_restart(&desktop, &node).await;
    assert!(seen.contains("got[still-here]"), "{seen}");
}

#[cfg(unix)]
#[tokio::test]
async fn an_agent_syncs_a_project_and_sends_a_file_with_no_server() {
    let pair = synced_pair_with_agent(true).await;
    let sent = pair.work.path().join("sent.jsonl");
    let mut mcp = agent_mcp(&pair.desktop, &pair.a, pair.work.path(), &sent).await;
    fs::write(pair.a_folder.join("plan.txt"), "the plan").expect("a file");
    has_text(&pair.b_folder.join("plan.txt"), "the plan").await;
    let (text, failed) = mcp
        .call(
            "sync_project",
            json!({ "project": "proj", "machine": "box-b" }),
        )
        .await;
    assert!(!failed && text.contains("box-b: in sync"), "{text}");
    // A file goes to the other machine's first shared folder.
    let note = pair.roots[0].path().join("note.txt");
    fs::write(&note, "for b").expect("a file");
    let (text, failed) = mcp
        .call(
            "send_files",
            json!({ "machine": "box-b", "paths": [note.display().to_string()] }),
        )
        .await;
    assert!(!failed, "{text}");
    assert_eq!(
        fs::read_to_string(pair.roots[1].path().join("note.txt")).expect("the file landed"),
        "for b"
    );
    // Like scp: from outside what it shares, to a new folder outside what
    // the other machine shares.
    let outside = TempDir::new().expect("a temp folder");
    let from = outside.path().join("site");
    fs::create_dir(&from).expect("a folder");
    fs::write(from.join("index.html"), "hello").expect("a file");
    let dest = outside.path().join("deep/dest");
    let (text, failed) = mcp
        .call(
            "send_files",
            json!({
                "machine": "box-b",
                "paths": [from.display().to_string()],
                "dest": dest.display().to_string(),
            }),
        )
        .await;
    assert!(!failed, "{text}");
    assert_eq!(
        fs::read_to_string(dest.join("site/index.html")).expect("the folder landed"),
        "hello"
    );
    // A missing file is the one thing it refuses.
    let (text, failed) = mcp
        .call(
            "send_files",
            json!({ "machine": "box-b", "paths": ["/no/such/file"] }),
        )
        .await;
    assert!(failed, "{text}");
}

#[cfg(unix)]
#[tokio::test]
async fn a_mesh_takes_over_a_family_only_once_it_holds_a_key_of_it() {
    let (desktop, mut links) = desktop_pushing().await;
    let shared = TempDir::new().expect("a temp folder");
    let kept = shared.path().join("kept");
    fs::create_dir(&kept).expect("the project folder");
    let mut node = Node::start(Config {
        server: Some(desktop.id()),
        files: vec![shared.path().to_owned()],
        saved_projects: vec![kept.clone()],
        ..Config::default()
    })
    .await;
    let conn = {
        node.join(&desktop.ticket(DeviceKind::Node).await).await;
        desktop.reach(node.id).await
    };
    let mut server = timeout(LIMIT, links.recv())
        .await
        .expect("the node linked to its server in time")
        .expect("the server saw the node");
    // The mesh holds no such keys: a project the server set stays running.
    node.logged("from the mesh: owns []").await;
    assert_eq!(sync_names(&conn).await, ["kept"]);
    // And the server's pushes are still taken.
    server
        .tx
        .send(&Down::Credentials(env_credentials(&[("SRV_ONLY", "one")])))
        .await
        .expect("the credentials are sent");
    node.logged("credentials: 1 env vars").await;
    assert_eq!(env_seen(&conn, ["SRV_ONLY", "NOT_SET"]).await, "got[one][]");
    // The mesh gets its first sync key: it owns sync from now on.
    let folder = shared.path().join("proj");
    fs::create_dir(&folder).expect("the project folder");
    let spec = SyncSpec {
        folders: [(node.id, folder.display().to_string())].into(),
        paused: false,
    };
    desktop
        .mesh
        .set(
            Key::Sync("proj".to_owned()),
            Some(spec.to_bytes().expect("a spec encodes")),
        )
        .await
        .expect("the spec is written");
    node.logged("from the mesh: sync proj with 0 peers").await;
    assert_eq!(sync_names(&conn).await, ["proj"]);
    // Credentials stay the server's until the mesh holds one.
    server
        .tx
        .send(&Down::Credentials(env_credentials(&[("SRV_ONLY", "two")])))
        .await
        .expect("the credentials are sent");
    node.logged("credentials: 1 env vars").await;
    assert_eq!(env_seen(&conn, ["SRV_ONLY", "NOT_SET"]).await, "got[two][]");
    // Deleting the last credential of the mesh empties the family.
    set_credential(&desktop, "gh", "MESH_VAR", "from-the-mesh").await;
    node.logged("from the mesh: 1 credentials").await;
    desktop
        .mesh
        .set(Key::Credential("gh".to_owned()), None)
        .await
        .expect("the credential is deleted");
    node.logged("from the mesh: 0 credentials").await;
    assert_eq!(env_seen(&conn, ["SRV_ONLY", "MESH_VAR"]).await, "got[][]");
}

#[tokio::test]
async fn a_sync_spec_that_stops_reading_keeps_its_last_good_value() {
    let mut pair = synced_pair().await;
    pair.desktop
        .mesh
        .set(Key::Sync("proj".to_owned()), Some(b"not a spec".to_vec()))
        .await
        .expect("the entry is written");
    pair.a.logged("ignored sync/proj in the mesh").await;
    let conn = pair.desktop.reach(pair.a.id).await;
    assert_eq!(sync_names(&conn).await, ["proj"]);
}

/// An adapter that takes any number of sessions, one per process: each
/// prompt ends its turn at once, except one that starts with `hang`, which
/// ends when the turn is cancelled.
const TREE_ADAPTER: &str = r#"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
while read -r line; do
  case "$line" in
    *'"initialize"'*) reply '{"protocolVersion":1,"agentCapabilities":{}}' ;;
    *'"session/new"'*) reply '{"sessionId":"s-'$$'"}' ;;
    *'"text":"hang'*) prompt=$(id) ;;
    *'"session/cancel"'*) [ -n "$prompt" ] && printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"cancelled"}}\n' "$prompt" ;;
    *'"session/prompt"'*) reply '{"stopReason":"end_turn"}' ;;
    *'"id"'*) reply '{}' ;;
  esac
done
"#;

/// An agent tool call from the session `session` of the machine on the
/// other end of `conn`'s peer: the text it got, and whether it failed.
async fn agent_tool(
    from: &Member,
    node: EndpointId,
    session: &str,
    tool: Tool,
    args: serde_json::Value,
) -> (bool, String) {
    let conn = from.reach(node).await;
    let (send, recv) = conn.mesh.open_bi(conn.id).await.expect("a stream opens");
    let (mut tx, mut rx) = link(send, recv);
    tx.send(&Open {
        protocol: TOOLS.to_owned(),
    })
    .await
    .expect("the open is sent");
    match timeout(LIMIT, rx.recv::<Opened>())
        .await
        .expect("the node answered in time")
        .expect("the answer reads")
    {
        Some(Opened::Ready {}) => {}
        other => panic!("{TOOLS} was not opened: {other:?}"),
    }
    tx.send(&ToolRequest {
        session: session.to_owned(),
        tool,
        args: args.to_string(),
        approved: Default::default(),
    })
    .await
    .expect("the call is sent");
    match timeout(LIMIT, rx.recv::<Reply>())
        .await
        .expect("the call answered in time")
        .expect("a reply reads")
    {
        Some(Reply::Done { text, failed }) => (failed, text),
        other => panic!("not done: {other:?}"),
    }
}

/// Follows `session` on the node as the desktop does, until `done` says an
/// event ends the wait.
async fn follow(conn: &Link, session: &str, done: impl Fn(&AgentEvent) -> bool) -> Stream {
    let mut stream = ready(conn, "codex/1").await;
    stream
        .tx
        .send(&AgentRequest::Attach {
            session: session.to_owned(),
        })
        .await
        .expect("the attach is sent");
    timeout(LIMIT, async {
        loop {
            match stream
                .rx
                .recv::<AgentEvent>()
                .await
                .expect("an event reads")
            {
                Some(event) if done(&event) => return,
                Some(_) => {}
                None => panic!("the stream closed"),
            }
        }
    })
    .await
    .expect("the session got there in time");
    stream
}

#[cfg(unix)]
#[tokio::test]
async fn an_agent_on_another_machine_starts_a_child_here_and_reaches_only_its_own() {
    let desktop = desktop(None).await;
    let work = TempDir::new().expect("a temp folder");
    let node = Node::start(Config {
        agent: Some((
            "sh".to_owned(),
            vec![
                "-c".to_owned(),
                TREE_ADAPTER.to_owned(),
                "fake-adapter".to_owned(),
            ],
        )),
        ..Config::default()
    })
    .await;
    node.join(&desktop.ticket(DeviceKind::Node).await).await;
    let other = peer_node(&desktop).await;
    let seen = desktop.reach(node.id).await;
    let cwd = work.path().display().to_string();
    let link = |depth: u8| {
        json!({
            "machine": other.id(), "session": "p1",
            "root_machine": other.id(), "root": "p1",
            "depth": depth, "others": 0, "agent": "claude", "mode": null,
        })
    };
    let start = |prompt: &str, depth| {
        json!({ "harness": "codex", "prompt": prompt, "cwd": cwd,
                "model": null, "parent": link(depth) })
    };
    let child_id = |text: &str| -> String {
        serde_json::from_str::<serde_json::Value>(text).expect("JSON")["id"]
            .as_str()
            .expect("an id")
            .to_owned()
    };

    // A child starts, linked to the session that asked.
    let (failed, text) =
        agent_tool(&other, node.id, "p1", Tool::StartAgent, start("work", 1)).await;
    assert!(!failed, "{text}");
    let id = child_id(&text);
    follow(&seen, &id, |event| match event {
        AgentEvent::TurnDone { .. } => true,
        AgentEvent::Attached { session } => session.turns >= 1,
        _ => false,
    })
    .await;
    let mut attach = ready(&seen, "codex/1").await;
    attach
        .tx
        .send(&AgentRequest::Attach {
            session: id.clone(),
        })
        .await
        .expect("the attach is sent");
    match timeout(LIMIT, attach.rx.recv::<AgentEvent>())
        .await
        .expect("the attach answered in time")
        .expect("an event reads")
    {
        Some(AgentEvent::Attached { session }) => {
            let parent = session.parent.expect("the child names its parent");
            assert_eq!((parent.session.as_str(), parent.depth), ("p1", 1));
            assert_eq!(parent.machine, Some(other.id()));
        }
        other => panic!("not attached: {other:?}"),
    }

    // Its parent reads it; another session of the same machine does not.
    let (failed, text) = agent_tool(
        &other,
        node.id,
        "p1",
        Tool::AgentStatus,
        json!({ "id": id }),
    )
    .await;
    assert!(!failed, "{text}");
    assert!(text.contains(r#""state":"idle""#), "{text}");
    let (failed, text) = agent_tool(
        &other,
        node.id,
        "p2",
        Tool::AgentStatus,
        json!({ "id": id }),
    )
    .await;
    assert!(failed && text.starts_with("You have no agent"), "{text}");
    let (failed, _) = agent_tool(&other, node.id, "p2", Tool::StopAgent, json!({ "id": id })).await;
    assert!(failed);
    // A link that names another machine than the caller is refused.
    let mut forged = start("work", 1);
    forged["parent"]["machine"] = json!(node.id);
    let (failed, text) = agent_tool(&other, node.id, "p1", Tool::StartAgent, forged).await;
    assert!(failed && text.contains("another machine"), "{text}");

    // The depth the caller passes is checked on this machine.
    let (failed, text) =
        agent_tool(&other, node.id, "p1", Tool::StartAgent, start("deep", 4)).await;
    assert!(failed && text.contains("3 levels deep"), "{text}");
    // So is the count of the root's agents: eight run at most, the one
    // running here among them.
    let mut running = vec![id.clone()];
    for at in 1..8 {
        let (failed, text) = agent_tool(
            &other,
            node.id,
            "p1",
            Tool::StartAgent,
            start(&format!("fill {at}"), 1),
        )
        .await;
        assert!(!failed, "{text}");
        running.push(child_id(&text));
    }
    let (failed, text) = agent_tool(&other, node.id, "p1", Tool::StartAgent, start("two", 1)).await;
    assert!(failed && text.contains("at most 8 agents"), "{text}");
    // One that stops leaves room.
    let gone = running.pop().expect("a child");
    let (failed, text) = agent_tool(
        &other,
        node.id,
        "p1",
        Tool::StopAgent,
        json!({ "id": gone }),
    )
    .await;
    assert!(!failed, "{text}");

    // Stop reaches a working child's turn, then its session.
    let (failed, text) =
        agent_tool(&other, node.id, "p1", Tool::StartAgent, start("hang on", 1)).await;
    assert!(!failed, "{text}");
    let hanging = child_id(&text);
    let working = beep_proto::node::agent::Status::Working;
    let mut stream = follow(&seen, &hanging, |event| match event {
        AgentEvent::State { state } => state.status == working,
        AgentEvent::Attached { session } => session.state.status == working,
        _ => false,
    })
    .await;
    let (failed, text) = agent_tool(
        &other,
        node.id,
        "p1",
        Tool::StopAgent,
        json!({ "id": hanging, "turn_only": true }),
    )
    .await;
    assert!(!failed, "{text}");
    timeout(LIMIT, async {
        loop {
            match stream
                .rx
                .recv::<AgentEvent>()
                .await
                .expect("an event reads")
            {
                Some(AgentEvent::TurnDone { .. }) => return,
                Some(_) => {}
                None => panic!("the stream closed"),
            }
        }
    })
    .await
    .expect("the turn stopped in time");
    let (failed, text) = agent_tool(
        &other,
        node.id,
        "p1",
        Tool::StopAgent,
        json!({ "id": hanging }),
    )
    .await;
    assert!(!failed, "{text}");
    timeout(LIMIT, async {
        loop {
            match stream
                .rx
                .recv::<AgentEvent>()
                .await
                .expect("an event reads")
            {
                Some(AgentEvent::Ended { .. }) => return,
                Some(_) => {}
                None => return,
            }
        }
    })
    .await
    .expect("the session ended in time");
}

#[cfg(unix)]
#[tokio::test]
async fn a_root_may_not_have_more_than_eight_agents_over_two_machines() {
    let desktop = desktop(None).await;
    let work = TempDir::new().expect("a temp folder");
    let config = || Config {
        agent: Some((
            "sh".to_owned(),
            vec![
                "-c".to_owned(),
                TREE_ADAPTER.to_owned(),
                "fake-adapter".to_owned(),
            ],
        )),
        ..Config::default()
    };
    let (a, b) = (Node::start(config()).await, Node::start(config()).await);
    a.join(&desktop.ticket(DeviceKind::Node).await).await;
    b.join(&desktop.ticket(DeviceKind::Node).await).await;
    let other = peer_node(&desktop).await;
    let cwd = work.path().display().to_string();
    let start = |prompt: &str| {
        json!({ "harness": "codex", "prompt": prompt, "cwd": cwd, "model": null,
                "parent": {
                    "machine": other.id(), "session": "p1",
                    "root_machine": other.id(), "root": "p1",
                    "depth": 1, "others": 0, "agent": "claude", "mode": null,
                } })
    };
    let id_of = |text: &str| -> String {
        serde_json::from_str::<serde_json::Value>(text).expect("JSON")["id"]
            .as_str()
            .expect("an id")
            .to_owned()
    };
    // Four on each machine: neither is past eight alone.
    let mut on_b = Vec::new();
    for at in 0..4 {
        let (failed, text) = agent_tool(
            &other,
            a.id,
            "p1",
            Tool::StartAgent,
            start(&format!("a {at}")),
        )
        .await;
        assert!(!failed, "{text}");
        let (failed, text) = agent_tool(
            &other,
            b.id,
            "p1",
            Tool::StartAgent,
            start(&format!("b {at}")),
        )
        .await;
        assert!(!failed, "{text}");
        on_b.push(id_of(&text));
    }
    // A ninth is refused, once the two machines have found each other: one
    // that starts before then is stopped and tried again.
    let refused = timeout(LIMIT, async {
        loop {
            let (failed, text) =
                agent_tool(&other, b.id, "p1", Tool::StartAgent, start("ninth")).await;
            if failed {
                return text;
            }
            let (failed, text) = agent_tool(
                &other,
                b.id,
                "p1",
                Tool::StopAgent,
                json!({ "id": id_of(&text) }),
            )
            .await;
            assert!(!failed, "{text}");
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .expect("the nodes counted each other in time");
    assert!(refused.contains("at most 8 agents"), "{refused}");
    let (failed, text) = agent_tool(&other, a.id, "p1", Tool::StartAgent, start("ninth")).await;
    assert!(failed && text.contains("at most 8 agents"), "{text}");
    // One that stops on b makes room on a.
    let (failed, text) = agent_tool(
        &other,
        b.id,
        "p1",
        Tool::StopAgent,
        json!({ "id": on_b[0] }),
    )
    .await;
    assert!(!failed, "{text}");
    let (failed, text) = agent_tool(&other, a.id, "p1", Tool::StartAgent, start("fits")).await;
    assert!(!failed, "{text}");
}

/// A node that was stopped: its folder and fake home are kept.
struct Stopped {
    dir: TempDir,
    home: TempDir,
    id: EndpointId,
    env: Vec<(&'static str, String)>,
}

impl Stopped {
    async fn start(self) -> Node {
        Node::run(self.dir, self.home, self.id, self.env).await
    }
}

fn adapter_of(script: &str, log: &Path) -> (String, Vec<String>) {
    (
        "sh".to_owned(),
        vec![
            "-c".to_owned(),
            script.to_owned(),
            "fake-adapter".to_owned(),
            log.display().to_string(),
        ],
    )
}

/// `a` and `b` run a fake coding agent, the script given; each logs to
/// `sent.jsonl` and `sent-b.jsonl` in `work`.
async fn synced_pair_with(agent: Option<&str>, agent_b: Option<&str>) -> Pair {
    synced_pair_env(agent, agent_b, &[]).await
}

/// As `synced_pair_with`, both nodes running with `env` as well.
async fn synced_pair_env(
    agent: Option<&str>,
    agent_b: Option<&str>,
    env: &[(&'static str, String)],
) -> Pair {
    let work = TempDir::new().expect("a temp folder");
    let desktop = desktop(None).await;
    let roots = [
        TempDir::new().expect("a temp folder"),
        TempDir::new().expect("a temp folder"),
    ];
    let sent = work.path().join("sent.jsonl");
    let mut a = Node::start(Config {
        name: Some("box-a"),
        files: vec![roots[0].path().to_owned()],
        agent: agent.map(|script| adapter_of(script, &sent)),
        env: env.to_vec(),
        ..Config::default()
    })
    .await;
    let mut b = Node::start(Config {
        name: Some("box-b"),
        files: vec![roots[1].path().to_owned()],
        agent: agent_b.map(|script| adapter_of(script, &work.path().join("sent-b.jsonl"))),
        env: env.to_vec(),
        ..Config::default()
    })
    .await;
    a.join(&desktop.ticket(DeviceKind::Node).await).await;
    b.join(&desktop.ticket(DeviceKind::Node).await).await;
    let a_folder = roots[0].path().join("proj");
    let b_folder = roots[1].path().join("proj");
    fs::create_dir(&a_folder).expect("a project folder");
    fs::create_dir(&b_folder).expect("a project folder");
    let spec = SyncSpec {
        folders: [
            (a.id, a_folder.display().to_string()),
            (b.id, b_folder.display().to_string()),
        ]
        .into(),
        paused: false,
    };
    desktop
        .mesh
        .set(
            Key::Sync("proj".to_owned()),
            Some(spec.to_bytes().expect("a spec encodes")),
        )
        .await
        .expect("the spec is written");
    a.logged("from the mesh: sync proj").await;
    b.logged("from the mesh: sync proj").await;
    Pair {
        desktop,
        a,
        b,
        a_folder,
        b_folder,
        roots,
        work,
    }
}

/// A coding agent in shell that can load sessions and logs each line it is
/// sent to `$1`.
const MOVE_ADAPTER: &str = r#"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
while read -r line; do
  printf '%s\n' "$line" >> "$1"
  case "$line" in
    *'"initialize"'*) reply '{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}' ;;
    *'"session/new"'*) reply '{"sessionId":"s"}' ;;
    *'"session/load"'*) reply '{}' ;;
    *'"session/prompt"'*) reply '{"stopReason":"end_turn"}' ;;
    *'"method"'*'"id"'*|*'"id"'*'"method"'*) reply '{}' ;;
  esac
done
"#;

const MOVED_ID: &str = "s";

const ROLLOUT: &str = "rollout-2026-11-04T09-30-00-s.jsonl";

/// Codex's rollout for session `s` as it lies on a node that ran it in `folder`.
fn rollout_on(node: &Node, folder: &Path) -> PathBuf {
    let path = node
        .home
        .path()
        .join(".codex/sessions/2026/11/04")
        .join(ROLLOUT);
    fs::create_dir_all(path.parent().expect("a folder")).expect("the day's folder");
    let cwd = folder.display().to_string();
    fs::write(
        &path,
        format!(
            "{}\n{}\n{}\n",
            json!({"timestamp":"2026-11-04T09:30:00.000Z","type":"session_meta","payload":{"id":MOVED_ID,"cwd":cwd,"runtime_workspace_roots":[cwd]}}),
            json!({"timestamp":"2026-11-04T09:30:01.000Z","type":"turn_context","payload":{"cwd":cwd,"model":"m"}}),
            json!({"timestamp":"2026-11-04T09:30:02.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":format!("look at {cwd}/main.rs")}]}}),
        ),
    )
    .expect("a rollout");
    path
}

/// What the desktop writes to move session `s` from `a` to `b`.
async fn ask_to_move(desktop: &Member, from: EndpointId, to: EndpointId, stop: bool) {
    ask_to_move_in(desktop, from, to, stop, "").await;
}

/// As `ask_to_move`, for a session in `path` below the project's folder.
async fn ask_to_move_in(
    desktop: &Member,
    from: EndpointId,
    to: EndpointId,
    stop: bool,
    path: &str,
) {
    let record = SessionMove {
        agent: "codex".into(),
        id: MOVED_ID.to_owned(),
        from,
        to,
        project: "proj".to_owned(),
        path: path.to_owned(),
        mode: None,
        stop,
        at: 1_793_696_400_000,
        worktree: None,
    };
    desktop
        .mesh
        .set(
            Key::SessionMove(MOVED_ID.to_owned()),
            Some(record.to_bytes().expect("a record encodes")),
        )
        .await
        .expect("the record is written");
}

/// How session `s`'s move stands, as the node it moves to says.
async fn progress_on(conn: &Link) -> Option<beep_proto::node::agent::MoveProgress> {
    let mut asked = ready(conn, SESSIONS_PROTOCOL).await;
    asked
        .tx
        .send(&SessionsRequest::MoveProgress {
            session: MOVED_ID.to_owned(),
        })
        .await
        .expect("the request is sent");
    match timeout(LIMIT, asked.rx.recv::<SessionsReply>())
        .await
        .expect("the node answered in time")
        .expect("the answer reads")
    {
        Some(SessionsReply::Moving { progress }) => progress,
        other => panic!("not a move answer: {other:?}"),
    }
}

/// Waits for the fake agent to have been sent a line containing `text`.
async fn line_sent(log: &Path, text: &str) -> String {
    timeout(LIMIT, async {
        loop {
            let read = fs::read_to_string(log).unwrap_or_default();
            if let Some(line) = read.lines().find(|line| line.contains(text)) {
                return line.to_owned();
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("the agent was never sent {text:?}"))
}

/// Waits until the move of `s` stands where `done` says.
async fn until_progress(
    conn: &Link,
    what: &str,
    done: impl Fn(&beep_proto::node::agent::MoveProgress) -> bool,
) {
    timeout(LIMIT, async {
        loop {
            if progress_on(conn).await.is_some_and(|p| done(&p)) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("the move never reached: {what}"));
}

#[cfg(unix)]
#[tokio::test]
async fn a_running_session_moves_to_the_other_machine_with_its_conversation_and_files() {
    use std::os::unix::fs::PermissionsExt;
    let mut pair = synced_pair_with(Some(MOVE_ADAPTER), Some(MOVE_ADAPTER)).await;
    fs::write(pair.a_folder.join("main.rs"), "fn main() {}\n").expect("a file");
    let conn_a = pair.desktop.reach(pair.a.id).await;
    let conn_b = pair.desktop.reach(pair.b.id).await;
    // The session runs on `a`, in its checkout.
    let mut agent = ready(&conn_a, "codex/1").await;
    agent
        .tx
        .send(&AgentRequest::Start {
            cwd: Some(pair.a_folder.display().to_string()),
            prompt: "go".to_owned(),
            model: None,
            effort: None,
            checkout: None,
        })
        .await
        .expect("the start is sent");
    timeout(LIMIT, async {
        loop {
            match agent.rx.recv::<AgentEvent>().await.expect("an event reads") {
                Some(AgentEvent::TurnDone { .. }) => return,
                Some(AgentEvent::Ended { reason }) => panic!("the session ended: {reason}"),
                Some(_) => {}
                None => panic!("the stream closed"),
            }
        }
    })
    .await
    .expect("the turn finished in time");
    let source_rollout = rollout_on(&pair.a, &pair.a_folder);

    ask_to_move(&pair.desktop, pair.a.id, pair.b.id, false).await;
    pair.a
        .logged("the move of session s is done on this side")
        .await;
    pair.b.logged("took over codex session s").await;

    // The target stores the conversation for its own checkout, privately,
    // with every other byte as the source had it.
    let b_rollout = pair
        .b
        .home
        .path()
        .join(".codex/sessions/2026/11/04")
        .join(ROLLOUT);
    let moved = fs::read_to_string(&b_rollout).expect("the transcript is in the store");
    let (from, to) = (
        pair.a_folder.display().to_string(),
        pair.b_folder.display().to_string(),
    );
    let original = fs::read_to_string(&source_rollout).expect("the source keeps its own");
    assert!(original.contains(&from) && !original.contains(&to));
    assert!(moved.contains(&to));
    assert_eq!(
        moved.lines().last(),
        original.lines().last(),
        "a line with no folder field is as it was"
    );
    assert!(
        !moved.lines().take(2).any(|line| line.contains(&from)),
        "{moved}"
    );
    assert_eq!(
        fs::metadata(&b_rollout)
            .expect("it exists")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    // The files had arrived before the session resumed.
    assert_eq!(
        fs::read_to_string(pair.b_folder.join("main.rs")).expect("synced"),
        "fn main() {}\n"
    );
    // It resumed the session in its own checkout.
    let load = line_sent(&pair.work.path().join("sent-b.jsonl"), "session/load").await;
    assert!(
        load.contains(r#""sessionId":"s""#) && load.contains(&to),
        "{load}"
    );
    assert!(!load.contains(&from), "{load}");
    until_progress(&conn_b, "done", |p| p.phase == MovePhase::Done).await;
    // The source ended it, and keeps the transcript.
    let mut list = ready(&conn_a, SESSIONS_PROTOCOL).await;
    list.tx.send(&SessionsRequest::List {}).await.expect("sent");
    match timeout(LIMIT, list.rx.recv::<SessionsReply>())
        .await
        .expect("answered")
        .expect("reads")
    {
        Some(SessionsReply::Sessions { sessions }) => {
            assert!(
                sessions.iter().all(|s| s.state.status == Status::Ended),
                "{sessions:?}"
            );
        }
        other => panic!("not a list: {other:?}"),
    }
    assert!(source_rollout.exists());
    // The first prompt tells the agent the session moved and from where.
    let mut follow = ready(&conn_b, "codex/1").await;
    follow
        .tx
        .send(&AgentRequest::Attach {
            session: MOVED_ID.to_owned(),
        })
        .await
        .expect("the attach is sent");
    follow
        .tx
        .send(&AgentRequest::Prompt {
            text: "continue".to_owned(),
        })
        .await
        .expect("the prompt is sent");
    timeout(LIMIT, async {
        loop {
            if let Some(AgentEvent::TurnDone { .. }) = follow
                .rx
                .recv::<AgentEvent>()
                .await
                .expect("an event reads")
            {
                return;
            }
        }
    })
    .await
    .expect("the turn finished in time");
    let log = fs::read_to_string(pair.work.path().join("sent-b.jsonl")).expect("the agent's log");
    let prompt = log
        .lines()
        .find(|line| line.contains("session/prompt") && line.contains("continue"))
        .expect("the prompt reached the agent");
    assert!(
        prompt.contains("moved from another machine") && prompt.contains(&from),
        "{prompt}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_move_waits_for_a_source_that_is_offline_and_goes_on_when_it_returns() {
    let Pair {
        desktop,
        a,
        mut b,
        a_folder,
        b_folder,
        work,
        roots: _roots,
    } = synced_pair_with(None, Some(MOVE_ADAPTER)).await;
    fs::write(a_folder.join("main.rs"), "fn main() {}\n").expect("a file");
    rollout_on(&a, &a_folder);
    has_text(&b_folder.join("main.rs"), "fn main() {}\n").await;
    let conn_b = desktop.reach(b.id).await;
    let (a_id, b_id) = (a.id, b.id);
    let down = a.stop().await;

    ask_to_move(&desktop, a_id, b_id, false).await;
    until_progress(&conn_b, "waiting for the source", |p| {
        p.phase == MovePhase::Waiting && p.source_away
    })
    .await;
    let b_rollout = b
        .home
        .path()
        .join(".codex/sessions/2026/11/04")
        .join(ROLLOUT);
    assert!(
        !b_rollout.exists(),
        "nothing is stored before the source answers"
    );
    let log = work.path().join("sent-b.jsonl");
    assert!(
        !fs::read_to_string(&log).is_ok_and(|log| log.contains("session/load")),
        "the session did not start on a guess"
    );

    let _a = down.start().await;
    b.logged("took over codex session s").await;
    let moved = fs::read_to_string(&b_rollout).expect("the transcript is in the store");
    let header: Vec<_> = moved.lines().take(2).collect();
    assert!(
        header
            .iter()
            .all(|line| line.contains(&b_folder.display().to_string()))
    );
    assert!(
        !header
            .iter()
            .any(|line| line.contains(&a_folder.display().to_string()))
    );
    until_progress(&conn_b, "done", |p| p.phase == MovePhase::Done).await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_machine_that_cannot_take_the_session_leaves_it_running_where_it_was() {
    // `b` has no folder for the session: it gives up the move before `a`
    // ends anything.
    let mut pair = synced_pair_with(Some(MOVE_ADAPTER), None).await;
    let conn_a = pair.desktop.reach(pair.a.id).await;
    let mut agent = ready(&conn_a, "codex/1").await;
    agent
        .tx
        .send(&AgentRequest::Start {
            cwd: Some(pair.a_folder.display().to_string()),
            prompt: "go".to_owned(),
            model: None,
            effort: None,
            checkout: None,
        })
        .await
        .expect("the start is sent");
    timeout(LIMIT, async {
        loop {
            match agent.rx.recv::<AgentEvent>().await.expect("an event reads") {
                Some(AgentEvent::TurnDone { .. }) => return,
                Some(AgentEvent::Ended { reason }) => panic!("the session ended: {reason}"),
                Some(_) => {}
                None => panic!("the stream closed"),
            }
        }
    })
    .await
    .expect("the turn finished in time");
    rollout_on(&pair.a, &pair.a_folder);

    ask_to_move_in(&pair.desktop, pair.a.id, pair.b.id, false, "missing").await;
    pair.a.logged("the move of session s did not finish").await;
    // `a` still runs the session, and takes a message again.
    let mut follow = ready(&conn_a, "codex/1").await;
    follow
        .tx
        .send(&AgentRequest::Attach {
            session: MOVED_ID.to_owned(),
        })
        .await
        .expect("the attach is sent");
    follow
        .tx
        .send(&AgentRequest::Prompt {
            text: "carry on".to_owned(),
        })
        .await
        .expect("the prompt is sent");
    line_sent(&pair.work.path().join("sent.jsonl"), "carry on").await;
    let mut list = ready(&conn_a, SESSIONS_PROTOCOL).await;
    list.tx.send(&SessionsRequest::List {}).await.expect("sent");
    match timeout(LIMIT, list.rx.recv::<SessionsReply>())
        .await
        .expect("answered")
        .expect("reads")
    {
        Some(SessionsReply::Sessions { sessions }) => {
            assert!(
                sessions.iter().any(|s| s.state.status != Status::Ended),
                "{sessions:?}"
            );
        }
        other => panic!("not a list: {other:?}"),
    }
    let conn_b = pair.desktop.reach(pair.b.id).await;
    until_progress(&conn_b, "failed", |p| {
        p.phase == MovePhase::Failed
            && p.reason
                .as_deref()
                .is_some_and(|reason| reason.contains("no checkout"))
    })
    .await;
}

/// Runs git in `dir`, as a test user, and gives what it printed.
fn git_in(dir: &Path, args: &[&str]) -> String {
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

/// Whether the checkout in `dir` has its `HEAD` at `commit`.
fn git_is_at(dir: &Path, commit: &str) -> bool {
    std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).trim() == commit)
}

/// What the desktop writes to move session `s` from `a` to `b`, out of the
/// worktree `from` on `branch`.
async fn ask_to_move_worktree(
    desktop: &Member,
    from: EndpointId,
    to: EndpointId,
    branch: &str,
    worktree: &Path,
    at: u64,
) {
    let record = SessionMove {
        agent: "codex".into(),
        id: MOVED_ID.to_owned(),
        from,
        to,
        project: "proj".to_owned(),
        path: String::new(),
        mode: None,
        stop: false,
        at: 1_793_696_400_000 + at,
        worktree: Some(beep_proto::node::mesh_state::MovedWorktree {
            branch: branch.to_owned(),
            from: worktree.display().to_string(),
        }),
    };
    desktop
        .mesh
        .set(
            Key::SessionMove(MOVED_ID.to_owned()),
            Some(record.to_bytes().expect("a record encodes")),
        )
        .await
        .expect("the record is written");
}

/// Starts a session on the node at `conn`, in `cwd`, and waits for its turn.
async fn run_session_in(conn: &Link, cwd: &Path) {
    let mut agent = ready(conn, "codex/1").await;
    agent
        .tx
        .send(&AgentRequest::Start {
            cwd: Some(cwd.display().to_string()),
            prompt: "go".to_owned(),
            model: None,
            effort: None,
            checkout: None,
        })
        .await
        .expect("the start is sent");
    timeout(LIMIT, async {
        loop {
            match agent.rx.recv::<AgentEvent>().await.expect("an event reads") {
                Some(AgentEvent::TurnDone { .. }) => return,
                Some(AgentEvent::Ended { reason }) => panic!("the session ended: {reason}"),
                Some(_) => {}
                None => panic!("the stream closed"),
            }
        }
    })
    .await
    .expect("the turn finished in time");
}

/// A repository in `folder` with `feat` checked out in a worktree under
/// `.worktrees/`, holding uncommitted work; gives the worktree.
fn repo_with_dirty_worktree(folder: &Path) -> PathBuf {
    git_in(folder, &["init", "-q", "--initial-branch=main"]);
    fs::write(folder.join("main.rs"), "fn main() {}\n").expect("a file");
    git_in(folder, &["add", "."]);
    git_in(folder, &["commit", "-q", "-m", "first"]);
    git_in(folder, &["branch", "feat"]);
    let worktree = folder.join(".worktrees/feat");
    git_in(
        folder,
        &[
            "worktree",
            "add",
            "-q",
            worktree.to_str().expect("utf8"),
            "feat",
        ],
    );
    fs::write(worktree.join("main.rs"), "fn main() { todo!() }\n").expect("a change");
    fs::write(worktree.join("notes.txt"), "not committed\n").expect("a new file");
    worktree
}

#[cfg(unix)]
#[tokio::test]
async fn a_session_in_a_worktree_moves_with_its_branch_and_uncommitted_work() {
    let mut pair = synced_pair_with(Some(MOVE_ADAPTER), Some(MOVE_ADAPTER)).await;
    let a_worktree = repo_with_dirty_worktree(&pair.a_folder);
    // The checkout and its branches reach `b`; the worktree folder does not.
    timeout(LIMIT, async {
        while !pair.b_folder.join(".git/refs/heads/feat").exists() {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the branch reached the other machine");
    let conn_a = pair.desktop.reach(pair.a.id).await;
    let conn_b = pair.desktop.reach(pair.b.id).await;
    run_session_in(&conn_a, &a_worktree).await;
    rollout_on(&pair.a, &a_worktree);

    ask_to_move_worktree(&pair.desktop, pair.a.id, pair.b.id, "feat", &a_worktree, 0).await;
    pair.b.logged("took over codex session s").await;
    until_progress(&conn_b, "done", |p| p.phase == MovePhase::Done).await;

    // `b` made a worktree on the same branch with the uncommitted work.
    let b_worktree = pair.b_folder.join(".worktrees/feat");
    assert_eq!(
        git_in(&b_worktree, &["symbolic-ref", "--short", "HEAD"]),
        "feat"
    );
    assert_eq!(
        fs::read_to_string(b_worktree.join("main.rs")).expect("the change came"),
        "fn main() { todo!() }\n"
    );
    assert_eq!(
        fs::read_to_string(b_worktree.join("notes.txt")).expect("the new file came"),
        "not committed\n"
    );
    let load = line_sent(&pair.work.path().join("sent-b.jsonl"), "session/load").await;
    assert!(load.contains(&b_worktree.display().to_string()), "{load}");
    // `a` lost nothing, and its worktree let go of the branch.
    assert_eq!(
        fs::read_to_string(a_worktree.join("notes.txt")).expect("kept"),
        "not committed\n"
    );
    timeout(LIMIT, async {
        while std::process::Command::new("git")
            .args(["symbolic-ref", "-q", "HEAD"])
            .current_dir(&a_worktree)
            .output()
            .expect("git runs")
            .status
            .success()
        {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the source's worktree was detached");
}

#[cfg(unix)]
#[tokio::test]
async fn a_machine_whose_syncthing_ignores_the_worktree_refuses_it_and_the_session_stays() {
    let mut pair = synced_pair_with(Some(MOVE_ADAPTER), Some(MOVE_ADAPTER)).await;
    let a_worktree = repo_with_dirty_worktree(&pair.a_folder);
    // `b` is kept by Syncthing, which is told to leave worktrees out.
    fs::create_dir(pair.b_folder.join(".stfolder")).expect("the marker");
    fs::write(pair.b_folder.join(".stignore"), ".worktrees\n").expect("the ignore file");
    let conn_a = pair.desktop.reach(pair.a.id).await;
    let conn_b = pair.desktop.reach(pair.b.id).await;
    // The picker's question gets the refusal before anything is written.
    let mut asked = ready(&conn_b, SESSIONS_PROTOCOL).await;
    asked
        .tx
        .send(&SessionsRequest::MoveCheck {
            project: "proj".to_owned(),
            branch: "feat".to_owned(),
        })
        .await
        .expect("the request is sent");
    match timeout(LIMIT, asked.rx.recv::<SessionsReply>())
        .await
        .expect("answered")
        .expect("reads")
    {
        Some(SessionsReply::MoveChecked { ignored }) => assert!(ignored),
        other => panic!("not a move check: {other:?}"),
    }
    // A record written anyway fails on `b` before `a` stops anything.
    run_session_in(&conn_a, &a_worktree).await;
    rollout_on(&pair.a, &a_worktree);
    ask_to_move_worktree(&pair.desktop, pair.a.id, pair.b.id, "feat", &a_worktree, 0).await;
    until_progress(&conn_b, "refused", |p| {
        p.phase == MovePhase::Failed
            && p.reason
                .as_deref()
                .is_some_and(|reason| reason.contains("Syncthing is set not to sync"))
    })
    .await;
    pair.a.logged("the move of session s did not finish").await;
    assert!(!pair.b_folder.join(".worktrees/feat").exists());
    // `a` still runs the session.
    let mut list = ready(&conn_a, SESSIONS_PROTOCOL).await;
    list.tx.send(&SessionsRequest::List {}).await.expect("sent");
    match timeout(LIMIT, list.rx.recv::<SessionsReply>())
        .await
        .expect("answered")
        .expect("reads")
    {
        Some(SessionsReply::Sessions { sessions }) => {
            assert!(
                sessions.iter().any(|s| s.state.status != Status::Ended),
                "{sessions:?}"
            );
        }
        other => panic!("not a list: {other:?}"),
    }
}

/// Two nodes of one mesh running the move adapter, with no `files` and no
/// `sync/proj` record: each has `proj` only in its place record, as a project
/// imported on one machine at a time does.
async fn placed_pair() -> Pair {
    let work = TempDir::new().expect("a temp folder");
    let desktop = desktop(None).await;
    let roots = [
        TempDir::new().expect("a temp folder"),
        TempDir::new().expect("a temp folder"),
    ];
    let mut a = Node::start(Config {
        name: Some("box-a"),
        agent: Some(adapter_of(MOVE_ADAPTER, &work.path().join("sent.jsonl"))),
        ..Config::default()
    })
    .await;
    let mut b = Node::start(Config {
        name: Some("box-b"),
        agent: Some(adapter_of(MOVE_ADAPTER, &work.path().join("sent-b.jsonl"))),
        ..Config::default()
    })
    .await;
    a.join(&desktop.ticket(DeviceKind::Node).await).await;
    b.join(&desktop.ticket(DeviceKind::Node).await).await;
    let a_folder = roots[0].path().join("proj");
    let b_folder = roots[1].path().join("proj");
    for (node, folder) in [(&a, &a_folder), (&b, &b_folder)] {
        fs::create_dir(folder).expect("a project folder");
        let place = Place {
            folder: folder.display().to_string(),
            imported_ms: 0,
            fork: None,
        };
        desktop
            .mesh
            .set(
                Key::Project(format!("proj/{}", node.id)),
                Some(place.to_bytes().expect("a place encodes")),
            )
            .await
            .expect("the place is written");
    }
    // Written after the places, so a node that logs it has them too.
    set_credential(&desktop, "placed", "PLACED", "yes").await;
    a.logged("from the mesh: 1 credentials").await;
    b.logged("from the mesh: 1 credentials").await;
    Pair {
        desktop,
        a,
        b,
        a_folder,
        b_folder,
        roots,
        work,
    }
}

/// A fake Syncthing GUI on `node`'s machine that keeps `folder` and has `need`
/// items still to get. Its events never come.
async fn fake_syncthing_status(
    node: &Node,
    folder: &Path,
    need: watch::Receiver<u64>,
) -> AbortOnDropHandle<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
    let addr = listener.local_addr().expect("an address");
    let home = if cfg!(target_os = "macos") {
        node.home
            .path()
            .join("Library/Application Support/Syncthing")
    } else {
        node.home.path().join("config/syncthing")
    };
    fs::create_dir_all(&home).expect("Syncthing's folder");
    fs::write(
        home.join("config.xml"),
        format!(
            r#"<configuration><gui tls="false"><address>{addr}</address><apikey>fake-key</apikey></gui></configuration>"#
        ),
    )
    .expect("the config");
    let listed = json!([{"id": "kept", "label": "Kept", "path": folder, "paused": false}]);
    AbortOnDropHandle::new(tokio::spawn(async move {
        let mut answering = JoinSet::new();
        while let Ok((stream, _)) = listener.accept().await {
            answering.spawn(fake_syncthing_answer(stream, need.clone(), listed.clone()));
        }
    }))
}

async fn fake_syncthing_answer(
    mut stream: TcpStream,
    need: watch::Receiver<u64>,
    listed: serde_json::Value,
) {
    let mut request = Vec::new();
    let mut buf = [0; 1024];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => request.extend_from_slice(&buf[..n]),
        }
    }
    let text = String::from_utf8_lossy(&request);
    let body = if text.starts_with("GET /rest/events") {
        std::future::pending::<()>().await;
        return;
    } else if text.starts_with("GET /rest/db/status?folder=kept ") {
        let need = *need.borrow();
        let state = if need == 0 { "idle" } else { "syncing" };
        json!({"needTotalItems": need, "state": state})
    } else {
        listed
    };
    let body = body.to_string();
    let reply = format!(
        "HTTP/1.0 200 OK\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(reply.as_bytes()).await.expect("the reply");
}

#[cfg(unix)]
#[tokio::test]
async fn a_session_moves_on_a_project_syncthing_keeps_with_no_sync_record_and_no_files() {
    let mut pair = placed_pair().await;
    let (need, told) = watch::channel(3);
    let _syncthing = fake_syncthing_status(&pair.b, &pair.b_folder, told).await;
    let conn_a = pair.desktop.reach(pair.a.id).await;
    let conn_b = pair.desktop.reach(pair.b.id).await;
    run_session_in(&conn_a, &pair.a_folder).await;
    rollout_on(&pair.a, &pair.a_folder);
    let b_rollout = pair
        .b
        .home
        .path()
        .join(".codex/sessions/2026/11/04")
        .join(ROLLOUT);

    ask_to_move(&pair.desktop, pair.a.id, pair.b.id, false).await;
    // `b` waits for its Syncthing and shows what is left.
    until_progress(&conn_b, "the wait for Syncthing", |p| {
        p.phase == MovePhase::Files && p.files_left == 3
    })
    .await;
    assert!(!b_rollout.exists(), "resumed before Syncthing finished");

    need.send_replace(0);
    pair.b.logged("took over codex session s").await;
    pair.a
        .logged("the move of session s is done on this side")
        .await;
    let moved = fs::read_to_string(&b_rollout).expect("the transcript is in the store");
    assert!(moved.contains(&pair.b_folder.display().to_string()));
}

#[cfg(unix)]
#[tokio::test]
async fn a_project_neither_beep_nor_syncthing_syncs_is_refused_before_the_source_stops() {
    let mut pair = placed_pair().await;
    let conn_a = pair.desktop.reach(pair.a.id).await;
    let conn_b = pair.desktop.reach(pair.b.id).await;
    run_session_in(&conn_a, &pair.a_folder).await;
    rollout_on(&pair.a, &pair.a_folder);

    ask_to_move(&pair.desktop, pair.a.id, pair.b.id, false).await;
    until_progress(&conn_b, "refused", |p| {
        p.phase == MovePhase::Failed
            && p.reason.as_deref()
                == Some("proj is not synced between machines: turn on sync on its page")
    })
    .await;
    pair.a.logged("the move of session s did not finish").await;
    // A session that had stopped would have been loaded again.
    let sent = fs::read_to_string(pair.work.path().join("sent.jsonl")).expect("the agent's log");
    assert!(!sent.contains("session/load"), "{sent}");
    let mut list = ready(&conn_a, SESSIONS_PROTOCOL).await;
    list.tx.send(&SessionsRequest::List {}).await.expect("sent");
    match timeout(LIMIT, list.rx.recv::<SessionsReply>())
        .await
        .expect("answered")
        .expect("reads")
    {
        Some(SessionsReply::Sessions { sessions }) => {
            assert!(
                sessions.iter().any(|s| s.state.status != Status::Ended),
                "{sessions:?}"
            );
        }
        other => panic!("not a list: {other:?}"),
    }
}

/// Both nodes copy every 200 ms, into a cache of `cache` bytes.
fn copy_env(cache: Option<u64>) -> Vec<(&'static str, String)> {
    let mut env = vec![("BEEP_SESSION_COPY_EVERY_MS", "200".to_owned())];
    if let Some(cache) = cache {
        env.push(("BEEP_SESSION_CACHE_BYTES", cache.to_string()));
    }
    env
}

/// Codex's rollout for session `id` as it lies on `node`, which ran it in
/// `folder`, last written `age` ago and `pad` bytes long at least.
fn rollout_for(node: &Node, folder: &Path, id: &str, age: Duration, pad: usize) -> PathBuf {
    let path = node
        .home
        .path()
        .join(".codex/sessions/2026/11/04")
        .join(format!("rollout-2026-11-04T09-30-00-{id}.jsonl"));
    fs::create_dir_all(path.parent().expect("a folder")).expect("the day's folder");
    let cwd = folder.display().to_string();
    fs::write(
        &path,
        format!(
            "{}\n{}\n",
            json!({"timestamp":"2026-11-04T09:30:00.000Z","type":"session_meta","payload":{"id":id,"cwd":cwd,"runtime_workspace_roots":[cwd]}}),
            json!({"timestamp":"2026-11-04T09:30:02.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"x".repeat(pad)}]}}),
        ),
    )
    .expect("a rollout");
    set_age(&path, age);
    path
}

fn set_age(path: &Path, age: Duration) {
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("the file opens")
        .set_modified(std::time::SystemTime::now() - age)
        .expect("the time is set");
}

/// What the node at `conn` keeps copies of.
async fn copies_on(conn: &Link) -> Vec<beep_proto::node::agent::CopyView> {
    let mut asked = ready(conn, SESSIONS_PROTOCOL).await;
    asked
        .tx
        .send(&SessionsRequest::Copies {})
        .await
        .expect("the request is sent");
    match timeout(LIMIT, asked.rx.recv::<SessionsReply>())
        .await
        .expect("answered")
        .expect("reads")
    {
        Some(SessionsReply::Copies { copies }) => copies,
        other => panic!("not a copies answer: {other:?}"),
    }
}

/// Waits until the copies on `conn` are what `done` says.
async fn until_copies(
    conn: &Link,
    what: &str,
    done: impl Fn(&[beep_proto::node::agent::CopyView]) -> bool,
) {
    timeout(LIMIT, async {
        loop {
            if done(&copies_on(conn).await) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("the copies never reached: {what}"));
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(read) = fs::read_dir(dir) {
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(files_under(&path));
            } else {
                out.push(path);
            }
        }
    }
    out
}

#[cfg(unix)]
#[tokio::test]
async fn a_machine_copies_the_sessions_of_the_mesh_in_the_background_and_keeps_them_out_of_the_store()
 {
    use std::os::unix::fs::PermissionsExt;
    let pair = synced_pair_env(None, Some(MOVE_ADAPTER), &copy_env(None)).await;
    let source = rollout_on(&pair.a, &pair.a_folder);
    let conn_b = pair.desktop.reach(pair.b.id).await;
    until_copies(&conn_b, "the session is copied", |c| {
        c.iter().any(|v| v.id == MOVED_ID && v.lost.is_none())
    })
    .await;
    let cached = pair.b.dir.path().join("sessions/codex/s/copy.jsonl");
    assert_eq!(
        fs::read(&cached).expect("the copy"),
        fs::read(&source).expect("the source")
    );
    assert_eq!(
        fs::metadata(&cached)
            .expect("it exists")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    // It waits in the cache: no harness store lists it, and nothing runs.
    assert!(
        files_under(&pair.b.home.path().join(".codex")).is_empty(),
        "{:?}",
        files_under(&pair.b.home.path().join(".codex"))
    );
    assert!(
        !fs::read_to_string(pair.work.path().join("sent-b.jsonl"))
            .is_ok_and(|log| log.contains("session/load"))
    );
    // A later turn on the source comes as only the new bytes.
    let before = fs::metadata(&cached).expect("it exists").len();
    let mut more = fs::OpenOptions::new()
        .append(true)
        .open(&source)
        .expect("the source opens");
    std::io::Write::write_all(
        &mut more,
        format!(
            "{}\n",
            json!({"type":"response_item","payload":{"type":"message","role":"assistant"}})
        )
        .as_bytes(),
    )
    .expect("a line is added");
    drop(more);
    until_copies(&conn_b, "the new turn is copied", |c| {
        c.iter().any(|v| v.id == MOVED_ID && v.size > before)
    })
    .await;
    assert_eq!(
        fs::read(&cached).expect("the copy"),
        fs::read(&source).expect("the source")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_session_continues_here_from_its_copy_while_its_machine_is_off() {
    let pair = synced_pair_env(None, Some(MOVE_ADAPTER), &copy_env(None)).await;
    fs::write(pair.a_folder.join("main.rs"), "fn main() {}\n").expect("a file");
    has_text(&pair.b_folder.join("main.rs"), "fn main() {}\n").await;
    rollout_on(&pair.a, &pair.a_folder);
    let conn_b = pair.desktop.reach(pair.b.id).await;
    until_copies(&conn_b, "the session is copied", |c| {
        c.iter().any(|v| v.id == MOVED_ID)
    })
    .await;
    let (b_folder, a_folder) = (pair.b_folder.clone(), pair.a_folder.clone());
    let _off = pair.a.stop().await;

    let mut asked = ready(&conn_b, SESSIONS_PROTOCOL).await;
    asked
        .tx
        .send(&SessionsRequest::ContinueCopy {
            agent: "codex".to_owned(),
            session: MOVED_ID.to_owned(),
            lost: None,
        })
        .await
        .expect("the request is sent");
    match timeout(LIMIT, asked.rx.recv::<SessionsReply>())
        .await
        .expect("answered")
        .expect("reads")
    {
        Some(SessionsReply::CopyDone {}) => {}
        other => panic!("not done: {other:?}"),
    }
    // It resumed in this machine's checkout, from the rewritten transcript.
    let load = line_sent(&pair.work.path().join("sent-b.jsonl"), "session/load").await;
    assert!(load.contains(&b_folder.display().to_string()), "{load}");
    let stored = fs::read_to_string(
        pair.b
            .home
            .path()
            .join(".codex/sessions/2026/11/04")
            .join(ROLLOUT),
    )
    .expect("the transcript is in the store");
    let header: Vec<_> = stored.lines().take(2).collect();
    assert!(
        header
            .iter()
            .all(|line| line.contains(&b_folder.display().to_string()))
    );
    assert!(
        !header
            .iter()
            .any(|line| line.contains(&a_folder.display().to_string()))
    );
    // The copy is spent: the session is a local one now.
    until_copies(&conn_b, "the copy is gone", |c| c.is_empty()).await;
}

#[cfg(unix)]
#[tokio::test]
async fn when_two_machines_continued_a_session_the_later_one_wins_and_the_other_stays_recoverable()
{
    let pair = synced_pair_env(None, None, &copy_env(None)).await;
    // `a` went on at its own pace, and `b` continued from a copy taken
    // earlier (activity 1 s into the epoch); `b`'s version is older.
    rollout_for(
        &pair.a,
        &pair.a_folder,
        MOVED_ID,
        Duration::from_secs(10),
        10,
    );
    let b_rollout = rollout_for(
        &pair.b,
        &pair.b_folder,
        MOVED_ID,
        Duration::from_secs(3600),
        10,
    );
    let marker = pair.b.dir.path().join("sessions/codex/s");
    fs::create_dir_all(&marker).expect("the cache folder");
    fs::write(marker.join("continued"), "1\n1").expect("the note");
    let conn_b = pair.desktop.reach(pair.b.id).await;
    until_copies(&conn_b, "b's version is kept as a copy", |c| {
        c.iter().any(|v| v.id == MOVED_ID && v.lost.is_some())
    })
    .await;
    assert!(!b_rollout.exists(), "the loser left the store");
    let lost = copies_on(&conn_b)
        .await
        .into_iter()
        .find(|v| v.lost.is_some())
        .expect("a recoverable copy");
    assert!(
        files_under(&pair.b.dir.path().join("sessions/codex/s"))
            .iter()
            .any(|f| f
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("lost-") && n.ends_with(".jsonl"))),
    );
    // The winner's version comes as an ordinary copy.
    until_copies(&conn_b, "a's version is copied", |c| {
        c.iter().any(|v| v.lost.is_none())
    })
    .await;
    // The user may throw the recoverable copy away.
    let mut asked = ready(&conn_b, SESSIONS_PROTOCOL).await;
    asked
        .tx
        .send(&SessionsRequest::DiscardCopy {
            agent: "codex".to_owned(),
            session: MOVED_ID.to_owned(),
            lost: lost.lost,
        })
        .await
        .expect("the request is sent");
    timeout(LIMIT, asked.rx.recv::<SessionsReply>())
        .await
        .expect("answered")
        .expect("reads");
    until_copies(&conn_b, "the lost version is gone", |c| {
        c.iter().all(|v| v.lost.is_none())
    })
    .await;
}

#[cfg(unix)]
#[tokio::test]
async fn the_cache_stays_under_its_cap_and_keeps_the_newest_sessions() {
    // A rollout is about 1.3 KB with its note: two fit, a third does not.
    let mut pair = synced_pair_env(None, None, &copy_env(Some(4000))).await;
    for (id, age) in [("s1", 3000), ("s2", 2000), ("s3", 1000)] {
        rollout_for(&pair.a, &pair.a_folder, id, Duration::from_secs(age), 1000);
    }
    let conn_b = pair.desktop.reach(pair.b.id).await;
    // The two newest come, whatever order a pass takes them in, and the
    // oldest is the one that does not fit.
    until_copies(&conn_b, "the two newest are copied", |c| {
        ["s2", "s3"].iter().all(|id| c.iter().any(|v| v.id == *id))
    })
    .await;
    pair.b
        .logged("not copying session s1: the cache is full")
        .await;
    let copies = copies_on(&conn_b).await;
    let mut ids: Vec<_> = copies.iter().map(|v| v.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, ["s2", "s3"], "the oldest is the one left out");
    let held: u64 = files_under(&pair.b.dir.path().join("sessions"))
        .iter()
        .map(|f| fs::metadata(f).map_or(0, |m| m.len()))
        .sum();
    assert!(held <= 4000, "{held} bytes are cached");
}

/// The reply of the node at `conn` to a request about the copy of session
/// `s`, or its version set aside at `lost`.
async fn copy_request(conn: &Link, discard: bool, lost: Option<u64>) -> SessionsReply {
    let mut asked = ready(conn, SESSIONS_PROTOCOL).await;
    let request = if discard {
        SessionsRequest::DiscardCopy {
            agent: "codex".to_owned(),
            session: MOVED_ID.to_owned(),
            lost,
        }
    } else {
        SessionsRequest::ContinueCopy {
            agent: "codex".to_owned(),
            session: MOVED_ID.to_owned(),
            lost,
        }
    };
    asked.tx.send(&request).await.expect("the request is sent");
    timeout(LIMIT, asked.rx.recv::<SessionsReply>())
        .await
        .expect("answered")
        .expect("reads")
        .expect("a reply")
}

/// Session `s` as `a` went on with it and as `b` continued it from an
/// earlier copy: `b`'s version is older and `b` holds the note that it
/// continued. Gives `b`'s transcript and waits until `b` set it aside.
async fn until_b_lost(pair: &Pair, conn_b: &Link) -> (PathBuf, u64) {
    rollout_for(
        &pair.a,
        &pair.a_folder,
        MOVED_ID,
        Duration::from_secs(10),
        10,
    );
    let b_rollout = rollout_for(
        &pair.b,
        &pair.b_folder,
        MOVED_ID,
        Duration::from_secs(3600),
        10,
    );
    let marker = pair.b.dir.path().join("sessions/codex/s");
    fs::create_dir_all(&marker).expect("the cache folder");
    fs::write(marker.join("continued"), "1\n1").expect("the note");
    until_copies(conn_b, "b's version is kept as a copy", |c| {
        c.iter().any(|v| v.id == MOVED_ID && v.lost.is_some())
    })
    .await;
    let lost = copies_on(conn_b)
        .await
        .into_iter()
        .find_map(|v| v.lost)
        .expect("a recoverable copy");
    (b_rollout, lost)
}

#[cfg(unix)]
#[tokio::test]
async fn a_restore_that_cannot_run_leaves_the_recoverable_copy_where_it_was() {
    // `b` has no Codex, so the restore is refused: nothing may change.
    let pair = synced_pair_env(None, None, &copy_env(None)).await;
    let conn_b = pair.desktop.reach(pair.b.id).await;
    let (_, lost) = until_b_lost(&pair, &conn_b).await;
    // The project's folder is not here (its disk is away): nothing to
    // continue in.
    let away = pair.b_folder.with_extension("away");
    fs::rename(&pair.b_folder, &away).expect("the folder goes");
    let reply = copy_request(&conn_b, false, Some(lost)).await;
    fs::rename(&away, &pair.b_folder).expect("the folder comes back");
    match reply {
        SessionsReply::Error { message } => {
            assert!(message.contains("no folder here"), "{message}")
        }
        other => panic!("not refused: {other:?}"),
    }
    let kept = copies_on(&conn_b).await;
    assert!(
        kept.iter().any(|v| v.lost == Some(lost)),
        "the version is still listed: {kept:?}"
    );
    assert!(
        files_under(&pair.b.dir.path().join("sessions/codex/s"))
            .iter()
            .any(|f| f.to_string_lossy().ends_with(&format!("lost-{lost}.jsonl")))
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_restore_over_another_version_sets_that_one_aside_under_its_own_name() {
    let pair = synced_pair_env(None, Some(MOVE_ADAPTER), &copy_env(None)).await;
    let conn_b = pair.desktop.reach(pair.b.id).await;
    let (b_rollout, lost) = until_b_lost(&pair, &conn_b).await;
    // The winner's transcript is in the store now, and does not run.
    let winner = format!(
        "{}\nthe winner\n",
        json!({"timestamp":"2026-11-04T09:30:00.000Z","type":"session_meta","payload":{"id":MOVED_ID,"cwd":pair.b_folder.display().to_string()}})
    );
    fs::write(&b_rollout, &winner).expect("a transcript");
    match copy_request(&conn_b, false, Some(lost)).await {
        SessionsReply::CopyDone {} => {}
        other => panic!("not done: {other:?}"),
    }
    // The transcript that was there is kept where the list shows it, as a
    // version that lost, and nothing is set aside unlisted in the store.
    let aside: Vec<_> = files_under(b_rollout.parent().expect("a folder"))
        .into_iter()
        .filter(|f| f.to_string_lossy().contains(".replaced-"))
        .collect();
    assert!(aside.is_empty(), "{aside:?}");
    let kept = copies_on(&conn_b).await;
    assert!(
        kept.iter().any(|v| v.lost.is_some_and(|at| at != lost)),
        "the winner's version is listed: {kept:?}"
    );
    let winner_kept = files_under(&pair.b.dir.path().join("sessions/codex/s"))
        .into_iter()
        .any(|f| {
            f.to_string_lossy().ends_with(".jsonl")
                && fs::read_to_string(&f).is_ok_and(|text| text == winner)
        });
    assert!(winner_kept);
    assert!(
        fs::read_to_string(&b_rollout)
            .expect("the restored version")
            .contains("session_meta")
    );
    until_copies(&conn_b, "the restored version left the list", |c| {
        c.iter().all(|v| v.lost != Some(lost))
    })
    .await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_session_a_move_names_is_not_filed_as_lost_by_the_note_of_an_earlier_continue() {
    let mut pair = synced_pair_env(None, None, &copy_env(None)).await;
    // The desktop moved `s` from `b` to `a`: the record decides the owner.
    ask_to_move(&pair.desktop, pair.b.id, pair.a.id, false).await;
    rollout_for(
        &pair.a,
        &pair.a_folder,
        MOVED_ID,
        Duration::from_secs(10),
        10,
    );
    let b_rollout = rollout_for(
        &pair.b,
        &pair.b_folder,
        MOVED_ID,
        Duration::from_secs(3600),
        10,
    );
    let marker = pair.b.dir.path().join("sessions/codex/s");
    fs::create_dir_all(&marker).expect("the cache folder");
    fs::write(marker.join("continued"), "1\n1").expect("the note");
    pair.b
        .logged("session s: not settled, a move names it")
        .await;
    assert!(b_rollout.exists(), "the transcript stayed in its store");
    let conn_b = pair.desktop.reach(pair.b.id).await;
    assert!(copies_on(&conn_b).await.iter().all(|v| v.lost.is_none()));
}

#[cfg(unix)]
#[tokio::test]
async fn a_branch_checked_out_on_the_target_refuses_the_move_and_the_session_stays() {
    let mut pair = synced_pair_with(Some(MOVE_ADAPTER), Some(MOVE_ADAPTER)).await;
    let a_worktree = repo_with_dirty_worktree(&pair.a_folder);
    timeout(LIMIT, async {
        while !pair.b_folder.join(".git/refs/heads/feat").exists() {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the branch reached the other machine");
    // On `b` the branch is checked out in a worktree of its own.
    let elsewhere = pair.b_folder.join(".worktrees/elsewhere");
    git_in(
        &pair.b_folder,
        &[
            "worktree",
            "add",
            "-q",
            elsewhere.to_str().expect("utf8"),
            "feat",
        ],
    );
    let conn_a = pair.desktop.reach(pair.a.id).await;
    let conn_b = pair.desktop.reach(pair.b.id).await;
    run_session_in(&conn_a, &a_worktree).await;
    rollout_on(&pair.a, &a_worktree);

    ask_to_move_worktree(&pair.desktop, pair.a.id, pair.b.id, "feat", &a_worktree, 1).await;
    until_progress(&conn_b, "refused", |p| {
        p.phase == MovePhase::Failed
            && p.reason
                .as_deref()
                .is_some_and(|reason| reason.contains("checked out in"))
    })
    .await;
    pair.a.logged("the move of session s did not finish").await;
    assert!(!pair.b_folder.join(".worktrees/feat").exists());
    // The source never ended the session, and still runs it.
    let mut list = ready(&conn_a, SESSIONS_PROTOCOL).await;
    list.tx.send(&SessionsRequest::List {}).await.expect("sent");
    match timeout(LIMIT, list.rx.recv::<SessionsReply>())
        .await
        .expect("answered")
        .expect("reads")
    {
        Some(SessionsReply::Sessions { sessions }) => {
            assert!(
                sessions.iter().any(|s| s.state.status != Status::Ended),
                "{sessions:?}"
            );
        }
        other => panic!("not a list: {other:?}"),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_session_in_a_worktree_moves_away_and_back_to_the_worktree_it_left() {
    let pair = synced_pair_with(Some(MOVE_ADAPTER), Some(MOVE_ADAPTER)).await;
    let a_worktree = repo_with_dirty_worktree(&pair.a_folder);
    timeout(LIMIT, async {
        while !pair.b_folder.join(".git/refs/heads/feat").exists() {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the branch reached the other machine");
    let conn_a = pair.desktop.reach(pair.a.id).await;
    let conn_b = pair.desktop.reach(pair.b.id).await;
    run_session_in(&conn_a, &a_worktree).await;
    rollout_on(&pair.a, &a_worktree);
    ask_to_move_worktree(&pair.desktop, pair.a.id, pair.b.id, "feat", &a_worktree, 1).await;
    until_progress(&conn_b, "done", |p| p.phase == MovePhase::Done).await;
    let b_worktree = pair.b_folder.join(".worktrees/feat");
    timeout(LIMIT, async {
        while std::process::Command::new("git")
            .args(["symbolic-ref", "-q", "HEAD"])
            .current_dir(&a_worktree)
            .output()
            .expect("git runs")
            .status
            .success()
        {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the source's worktree was detached");

    // What the session left in the folder went with it: the source's
    // worktree is clean, so the session can come back to it.
    timeout(LIMIT, async {
        while !git_in(&a_worktree, &["status", "--porcelain"]).is_empty() {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the source's worktree was cleaned once the work went");
    // The user writes something in it meanwhile: the move back is refused
    // before anything stops, naming the folder, and the session keeps
    // running on `b`.
    fs::write(a_worktree.join("scratch.txt"), "the user's\n").expect("a file");
    ask_to_move_worktree(&pair.desktop, pair.b.id, pair.a.id, "feat", &b_worktree, 2).await;
    until_progress(&conn_a, "refused for the folder", |p| {
        p.phase == MovePhase::Failed
            && p.reason
                .as_deref()
                .is_some_and(|reason| reason.contains(&a_worktree.display().to_string()))
    })
    .await;
    let mut list = ready(&conn_b, SESSIONS_PROTOCOL).await;
    list.tx.send(&SessionsRequest::List {}).await.expect("sent");
    match timeout(LIMIT, list.rx.recv::<SessionsReply>())
        .await
        .expect("answered")
        .expect("reads")
    {
        Some(SessionsReply::Sessions { sessions }) => {
            assert!(
                sessions.iter().any(|s| s.state.status != Status::Ended),
                "{sessions:?}"
            );
        }
        other => panic!("not a list: {other:?}"),
    }
    assert_eq!(
        fs::read_to_string(a_worktree.join("scratch.txt")).expect("untouched"),
        "the user's\n"
    );

    // Removed by the user, with a stash of the user's own on the checkout:
    // the folder is reused, and the stash is not touched.
    fs::remove_file(a_worktree.join("scratch.txt")).expect("removed");
    fs::write(pair.a_folder.join("main.rs"), "fn main() { mine() }\n").expect("a change");
    git_in(
        &pair.a_folder,
        &["stash", "push", "-q", "-m", "the user's own"],
    );
    let stash = git_in(&pair.a_folder, &["stash", "list"]);
    ask_to_move_worktree(&pair.desktop, pair.b.id, pair.a.id, "feat", &b_worktree, 3).await;
    until_progress(&conn_a, "done again", |p| p.phase == MovePhase::Done).await;
    assert_eq!(
        git_in(&a_worktree, &["symbolic-ref", "--short", "HEAD"]),
        "feat"
    );
    assert_eq!(
        fs::read_to_string(a_worktree.join("main.rs")).expect("the work came back"),
        "fn main() { todo!() }\n"
    );
    assert_eq!(git_in(&pair.a_folder, &["stash", "list"]), stash);
}

#[cfg(unix)]
#[tokio::test]
async fn a_session_that_moved_once_still_settles_a_later_two_way_continue() {
    let pair = synced_pair_env(None, None, &copy_env(None)).await;
    // The move of `s` from `b` to `a` ended long ago, on both sides.
    let old: u64 = 1_793_696_400_000;
    ask_to_move(&pair.desktop, pair.b.id, pair.a.id, false).await;
    let moves = pair.b.dir.path().join("moves");
    fs::create_dir_all(&moves).expect("the state folder");
    fs::write(
        moves.join("s.json"),
        format!(r#"{{"at":{old},"phase":"Done"}}"#),
    )
    .expect("the move is over");
    // Then `b` continued from a copy, and `a` went on too.
    rollout_for(
        &pair.a,
        &pair.a_folder,
        MOVED_ID,
        Duration::from_secs(10),
        10,
    );
    rollout_for(
        &pair.b,
        &pair.b_folder,
        MOVED_ID,
        Duration::from_secs(3600),
        10,
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after the epoch")
        .as_secs();
    let marker = pair.b.dir.path().join("sessions/codex/s");
    fs::create_dir_all(&marker).expect("the cache folder");
    fs::write(marker.join("continued"), format!("1\n{now}")).expect("the note");
    let conn_b = pair.desktop.reach(pair.b.id).await;
    until_copies(&conn_b, "the later version wins", |c| {
        c.iter().any(|v| v.id == MOVED_ID && v.lost.is_some())
    })
    .await;
}

/// Codex's rollout for session `id` that spent a million tokens.
fn rollout_with_usage(node: &Node, folder: &Path, id: &str) -> PathBuf {
    let path = node
        .home
        .path()
        .join(".codex/sessions/2026/11/04")
        .join(format!("rollout-2026-11-04T09-30-00-{id}.jsonl"));
    fs::create_dir_all(path.parent().expect("a folder")).expect("the day's folder");
    let cwd = folder.display().to_string();
    let used = json!({"input_tokens":1_000_000,"cached_input_tokens":0,"output_tokens":0,"total_tokens":1_000_000});
    fs::write(
        &path,
        format!(
            "{}\n{}\n{}\n",
            json!({"timestamp":"2026-11-04T09:30:00.000Z","type":"session_meta","payload":{"id":id,"cwd":cwd,"runtime_workspace_roots":[cwd]}}),
            json!({"timestamp":"2026-11-04T09:30:01.000Z","type":"turn_context","payload":{"cwd":cwd,"model":"gpt-6.1-sol"}}),
            json!({"timestamp":"2026-11-04T09:30:02.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":used,"last_token_usage":used}}}),
        ),
    )
    .expect("a rollout");
    path
}

#[cfg(unix)]
#[tokio::test]
async fn what_a_copy_spent_on_the_other_machine_is_not_counted_again_when_it_continues_here() {
    let pair = synced_pair_env(None, Some(MOVE_ADAPTER), &copy_env(None)).await;
    rollout_with_usage(&pair.a, &pair.a_folder, MOVED_ID);
    // A session of `b`'s own that spent too, to know its history was read.
    rollout_with_usage(&pair.b, &pair.b_folder, "mine");
    let conn_b = pair.desktop.reach(pair.b.id).await;
    until_copies(&conn_b, "the session is copied", |c| {
        c.iter().any(|v| v.id == MOVED_ID)
    })
    .await;
    match copy_request(&conn_b, false, None).await {
        SessionsReply::CopyDone {} => {}
        other => panic!("not done: {other:?}"),
    }
    let mut usage = ready(&conn_b, SESSIONS_PROTOCOL).await;
    usage
        .tx
        .send(&SessionsRequest::FollowUsage { history: true })
        .await
        .expect("the request is sent");
    let mut spent: Vec<String> = Vec::new();
    timeout(LIMIT, async {
        loop {
            match usage.rx.recv::<SessionsReply>().await.expect("reads") {
                Some(SessionsReply::SessionSpend { usage }) => {
                    spent.push(usage.session);
                    if spent.iter().any(|id| id == "mine") {
                        return;
                    }
                }
                Some(_) => {}
                None => panic!("the stream closed"),
            }
        }
    })
    .await
    .expect("b's own session was reported");
    // The rest of the history comes with it.
    let _ = timeout(Duration::from_millis(500), async {
        while let Ok(Some(reply)) = usage.rx.recv::<SessionsReply>().await {
            if let SessionsReply::SessionSpend { usage } = reply {
                spent.push(usage.session);
            }
        }
    })
    .await;
    assert!(
        !spent.iter().any(|id| id == MOVED_ID),
        "the machine that ran it counted that: {spent:?}"
    );
}

/// A coding agent that never ends a turn.
fn hanging_adapter() -> String {
    let adapter = MOVE_ADAPTER.replace(
        r#"    *'"session/prompt"'*) reply '{"stopReason":"end_turn"}' ;;"#,
        // A prompt is taken and never answered.
        r#"    *'"session/prompt"'*) : ;;"#,
    );
    assert_ne!(adapter, MOVE_ADAPTER, "the prompt line was found");
    adapter
}

#[cfg(unix)]
#[tokio::test]
async fn background_copies_wait_while_a_session_works_on_this_machine() {
    let mut pair = synced_pair_env(None, Some(&hanging_adapter()), &copy_env(None)).await;
    rollout_on(&pair.a, &pair.a_folder);
    let conn_b = pair.desktop.reach(pair.b.id).await;
    // A turn that does not end: the session works.
    let mut agent = ready(&conn_b, "codex/1").await;
    agent
        .tx
        .send(&AgentRequest::Start {
            cwd: Some(pair.b_folder.display().to_string()),
            prompt: "go".to_owned(),
            model: None,
            effort: None,
            checkout: None,
        })
        .await
        .expect("the start is sent");
    pair.b
        .logged("background copies wait while a session works")
        .await;
    assert!(
        copies_on(&conn_b).await.is_empty(),
        "nothing is copied meanwhile"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_nested_repository_among_the_new_files_refuses_the_move_before_the_session_stops() {
    let mut pair = synced_pair_with(Some(MOVE_ADAPTER), Some(MOVE_ADAPTER)).await;
    let a_worktree = repo_with_dirty_worktree(&pair.a_folder);
    let nested = a_worktree.join("vendor/lib");
    fs::create_dir_all(&nested).expect("a folder");
    git_in(&nested, &["init", "-q"]);
    fs::write(nested.join("f.txt"), "x\n").expect("a file");
    timeout(LIMIT, async {
        while !pair.b_folder.join(".git/refs/heads/feat").exists() {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the branch reached the other machine");
    let conn_a = pair.desktop.reach(pair.a.id).await;
    let conn_b = pair.desktop.reach(pair.b.id).await;
    run_session_in(&conn_a, &a_worktree).await;
    rollout_on(&pair.a, &a_worktree);
    ask_to_move_worktree(&pair.desktop, pair.a.id, pair.b.id, "feat", &a_worktree, 0).await;
    // Refused by name on the first read, with the session still running.
    until_progress(&conn_b, "refused by name", |p| {
        p.phase == MovePhase::Failed
            && p.reason
                .as_deref()
                .is_some_and(|reason| reason.contains("vendor/lib"))
    })
    .await;
    pair.a.logged("the move of session s did not finish").await;
    assert!(
        !pair
            .a
            .log
            .iter()
            .any(|line| line.contains("ended the session")),
        "the source stopped nothing"
    );
    assert!(!pair.b_folder.join(".worktrees/feat").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn a_machine_that_is_neither_end_of_an_old_move_still_settles_a_two_way_continue() {
    let pair = synced_pair_env(None, None, &copy_env(None)).await;
    // The record of an old move between two other machines is in the mesh;
    // `b` has no saved state for it.
    let record = SessionMove {
        agent: "codex".into(),
        id: MOVED_ID.to_owned(),
        from: SecretKey::generate().public(),
        to: SecretKey::generate().public(),
        project: "proj".to_owned(),
        path: String::new(),
        mode: None,
        stop: false,
        at: 1_793_696_400_000,
        worktree: None,
    };
    pair.desktop
        .mesh
        .set(
            Key::SessionMove(MOVED_ID.to_owned()),
            Some(record.to_bytes().expect("a record encodes")),
        )
        .await
        .expect("the record is written");
    rollout_for(
        &pair.a,
        &pair.a_folder,
        MOVED_ID,
        Duration::from_secs(10),
        10,
    );
    rollout_for(
        &pair.b,
        &pair.b_folder,
        MOVED_ID,
        Duration::from_secs(3600),
        10,
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after the epoch")
        .as_secs();
    let marker = pair.b.dir.path().join("sessions/codex/s");
    fs::create_dir_all(&marker).expect("the cache folder");
    fs::write(marker.join("continued"), format!("1\n{now}")).expect("the note");
    let conn_b = pair.desktop.reach(pair.b.id).await;
    until_copies(&conn_b, "the later version wins", |c| {
        c.iter().any(|v| v.id == MOVED_ID && v.lost.is_some())
    })
    .await;
}

/// `beep-node status` for the node running from `dir`.
async fn status_text(dir: &Path) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_beep-node"))
        .arg("status")
        .arg("--dir")
        .arg(dir)
        .kill_on_drop(true)
        .output();
    let out = timeout(LIMIT, out)
        .await
        .expect("status finished in time")
        .expect("status ran");
    assert!(
        out.status.success(),
        "status failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("status prints text")
}

/// The row of `status_text` that starts with `name`.
fn row<'a>(text: &'a str, name: &str) -> &'a str {
    text.lines()
        .find(|line| line.trim_start().starts_with(&format!("{name} ")))
        .unwrap_or("")
}

/// Asks `status` until `shown` holds of its text. A status has nothing to
/// wait on from outside; the outer limit is the only clock.
async fn until_status_shows(dir: &Path, shown: impl Fn(&str) -> bool) -> String {
    let (send, last) = watch::channel(String::new());
    let found = timeout(LIMIT, async {
        loop {
            let text = status_text(dir).await;
            if shown(&text) {
                return text;
            }
            send.send_replace(text);
            tokio::task::yield_now().await;
        }
    })
    .await;
    found.unwrap_or_else(|_| panic!("status never showed it; it last said:\n{}", *last.borrow()))
}

#[tokio::test]
async fn status_shows_the_devices_and_how_each_project_syncs() {
    let (a, root, folder) = bare_node().await;
    let mut pair = bare_nodes(a, root, folder).await;
    pair.sync().await;
    // A second project whose folder on a sits on a disk that is not there.
    let unmounted = pair._roots[0].path().join("unmounted/gone");
    let b_gone = pair.b_folder.with_file_name("gone");
    fs::create_dir(&b_gone).expect("a project folder");
    let spec = SyncSpec {
        folders: [
            (pair.a.id, unmounted.display().to_string()),
            (pair.b.id, b_gone.display().to_string()),
        ]
        .into(),
        paused: false,
    };
    pair.desktop
        .mesh
        .set(
            Key::Sync("gone".to_owned()),
            Some(spec.to_bytes().expect("a spec encodes")),
        )
        .await
        .expect("the spec is written");

    let dir = pair.a.dir.path().to_owned();
    let text = until_status_shows(&dir, |text| {
        row(text, "proj").contains("in sync")
            && row(text, "gone").contains("folder missing")
            && row(text, "box-b").contains("online")
    })
    .await;
    assert!(text.contains("Node      running for"), "{text}");
    assert!(row(&text, "desktop").contains("online"), "{text}");
    assert!(row(&text, "box-a").contains("this machine"), "{text}");
    assert!(text.contains("3 of 3 devices online"), "{text}");

    // b stops: a says when it last saw it.
    let _b = pair.b.stop().await;
    until_status_shows(&dir, |text| {
        row(text, "box-b").contains("offline, last seen")
    })
    .await;
}

/// Claude Code's answer to `get_usage` on a Max plan, as 2.1.318 gives it.
const CLAUDE_USAGE: &str = r#"{"type":"control_response","response":{"subtype":"success","request_id":"beep-usage","response":{"subscription_type":"max","rate_limits_available":true,"rate_limits":{"five_hour":null,"seven_day":{"utilization":73,"resets_at":"2026-11-10T01:00:00.022993+00:00","limit_dollars":null,"used_dollars":null},"seven_day_opus":null,"seven_day_sonnet":null,"extra_usage":{"is_enabled":false}},"limits":[{"kind":"weekly_all","group":"weekly","percent":73,"resets_at":"2026-11-10T01:00:00.022993+00:00","scope":null,"is_active":true}]},"behaviors":null}}"#;

/// Stand-ins for `claude` and `codex`, and their ACP adapters so the node
/// offers them, answering a plan read as the real CLIs do. Claude Code
/// takes `claude_secs` to answer, as it can when it starts.
#[cfg(unix)]
fn plan_clis(claude_secs: u32) -> TempDir {
    use std::os::unix::fs::PermissionsExt;
    let bin = TempDir::new().expect("a temp folder");
    let claude = format!(
        "#!/bin/sh\nread line\nsleep {claude_secs}\necho '{CLAUDE_USAGE}'\ncat >/dev/null\n"
    );
    let codex = r#"#!/bin/sh
read line
echo '{"id":1,"result":{"userAgent":"codex"}}'
read line
read line
echo '{"id":2,"result":{"rateLimits":{"primary":{"usedPercent":7,"windowDurationMins":10080,"resetsAt":1794272400},"secondary":null,"planType":"pro"}}}'
cat >/dev/null
"#;
    for (name, text) in [
        ("claude", claude.as_str()),
        ("codex", codex),
        ("claude-agent-acp", "#!/bin/sh\ncat >/dev/null\n"),
        ("codex-acp", "#!/bin/sh\ncat >/dev/null\n"),
    ] {
        let path = bin.path().join(name);
        fs::write(&path, text).expect("a script");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("executable");
    }
    bin
}

/// When each agent's plan came on a new `FollowUsage` stream, waiting up to
/// `within` for both.
async fn plans_came(conn: &Link, within: Duration) -> [Option<Duration>; 2] {
    let mut usage = ready(conn, SESSIONS_PROTOCOL).await;
    let start = std::time::Instant::now();
    usage
        .tx
        .send(&SessionsRequest::FollowUsage { history: true })
        .await
        .expect("the request is sent");
    let mut came = [None, None];
    let _ = timeout(within, async {
        while came.iter().any(Option::is_none) {
            match usage.rx.recv::<SessionsReply>().await.expect("reads") {
                Some(SessionsReply::PlanUsage { usage }) => {
                    let slot = if usage.agent.as_str() == "claude" {
                        0
                    } else {
                        1
                    };
                    came[slot].get_or_insert(start.elapsed());
                }
                Some(_) => {}
                None => panic!("the stream closed"),
            }
        }
    })
    .await;
    came
}

#[cfg(unix)]
#[tokio::test]
async fn a_slow_claude_code_neither_holds_back_codex_nor_the_next_look() {
    // Longer than the desktop listens to a quiet machine (`SETTLE`).
    let bin = plan_clis(4);
    let path = format!(
        "{}:{}",
        bin.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let desktop = desktop(None).await;
    let node = Node::start(Config {
        env: vec![("PATH", path)],
        ..Config::default()
    })
    .await;
    node.join(&desktop.ticket(DeviceKind::Node).await).await;
    let conn = desktop.reach(node.id).await;
    let quick = Duration::from_secs(2);
    // A desktop's look ends before Claude Code answers.
    let [claude, codex] = plans_came(&conn, quick).await;
    assert!(codex.is_some(), "Codex waited for Claude Code");
    assert!(claude.is_none(), "the stand-in answered early");
    // The read goes on without it, and the next stream hears the answer.
    let [claude, _] = plans_came(&conn, LIMIT).await;
    assert!(claude.is_some(), "Claude Code's plan came");
    // The look after that, as the desktop makes one each minute, has both at once.
    let [claude, codex] = plans_came(&conn, quick).await;
    assert!(
        claude.is_some(),
        "the Claude Code plan read before did not come at once"
    );
    assert!(
        codex.is_some(),
        "the Codex plan read before did not come at once"
    );
}

impl Node {
    /// Like `logged`, for a line that may have come before the last one
    /// waited for.
    async fn has_logged(&mut self, text: &str) {
        if !self.log.iter().any(|line| line.contains(text)) {
            self.logged(text).await;
        }
    }
}

/// Writes the skill `name` with `body` as its `SKILL.md` into the skills
/// folder `rel` under `home`.
fn put_skill(home: &Path, rel: &str, name: &str, body: &str) {
    let dir = home.join(rel).join(name);
    fs::create_dir_all(dir.join("refs")).expect("a skill folder");
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\n---\n{body}\n"),
    )
    .expect("the skill is written");
    fs::write(dir.join("refs/notes.md"), format!("{name} notes\n")).expect("a skill file");
}

/// Everything under `dir`, by path, for comparing folders.
fn tree(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(at) = stack.pop() {
        for entry in fs::read_dir(&at).expect("a folder").flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let data = fs::read(&path).expect("a file");
                out.push((path.strip_prefix(dir).expect("inside").to_path_buf(), data));
            }
        }
    }
    out.sort();
    out
}

#[tokio::test]
async fn a_users_skills_reach_the_other_node_and_differences_change_nothing() {
    let desktop = desktop(None).await;
    let a = Node::start(Config::default()).await;
    let mut b = Node::start(Config {
        name: Some("box-b"),
        ..Config::default()
    })
    .await;
    // On A: one skill in two agents' folders alike, one Beep installed.
    put_skill(a.home.path(), ".claude/skills", "notes", "Write notes.");
    put_skill(a.home.path(), ".agents/skills", "notes", "Write notes.");
    put_skill(
        a.home.path(),
        ".claude/skills",
        "beep-made",
        "<!-- Installed by Beep. Beep keeps this file current until you edit it. sha256:00 -->",
    );
    // On both: one alike, one that differs.
    for home in [a.home.path(), b.home.path()] {
        put_skill(home, ".claude/skills", "shared", "Same everywhere.");
    }
    put_skill(a.home.path(), ".claude/skills", "style", "Short lines.");
    put_skill(b.home.path(), ".claude/skills", "style", "Long lines.");
    let style_a = tree(&a.home.path().join(".claude/skills/style"));
    let style_b = tree(&b.home.path().join(".claude/skills/style"));

    a.join(&desktop.ticket(DeviceKind::Node).await).await;
    b.join(&desktop.ticket(DeviceKind::Node).await).await;
    let mut a = a;
    let differs = "Skill style differs on box-a and box-b; nothing changed";
    b.has_logged("skills: got notes from box-a").await;
    b.has_logged(differs).await;
    a.has_logged(differs).await;
    a.has_logged("1 other machines compared, 1 differ").await;

    let notes = tree(&a.home.path().join(".claude/skills/notes"));
    for rel in [".claude/skills/notes", ".agents/skills/notes"] {
        assert_eq!(tree(&b.home.path().join(rel)), notes, "{rel} on box-b");
    }
    assert!(!b.home.path().join(".claude/skills/beep-made").exists());
    assert!(!b.home.path().join(".agents/skills/beep-made").exists());
    assert_eq!(tree(&a.home.path().join(".claude/skills/style")), style_a);
    assert_eq!(tree(&b.home.path().join(".claude/skills/style")), style_b);
    assert!(!b.home.path().join(".agents/skills/style").exists());

    // Deleted on A: it stays on B, and A does not take it back.
    for rel in [".claude/skills/notes", ".agents/skills/notes"] {
        fs::remove_dir_all(a.home.path().join(rel)).expect("the skill is deleted");
    }
    let mut a = a.restart().await;
    a.logged("skills: 2 here, 1 other machines compared, 1 differ")
        .await;
    assert!(!a.home.path().join(".claude/skills/notes").exists());
    assert!(!a.home.path().join(".agents/skills/notes").exists());
    assert_eq!(tree(&b.home.path().join(".claude/skills/notes")), notes);

    // Told once each, and never for the copies alike.
    while let Ok(line) = b.lines.try_recv() {
        b.log.push(line);
    }
    for node in [&a, &b] {
        let told = |text: &str| node.log.iter().filter(|l| l.contains(text)).count();
        assert_eq!(told("Skill style differs"), 1, "{:#?}", node.log);
        assert_eq!(told("Skill shared"), 0);
        assert_eq!(told("Skill notes"), 0);
        assert_eq!(told("got notes"), usize::from(node.id == b.id));
    }

    // An edit on A reaches B, whole.
    put_skill(a.home.path(), ".claude/skills", "shared", "Edited on A.");
    let mut a = a.restart().await;
    b.has_logged("skills: updated shared from box-a").await;
    let shared = tree(&a.home.path().join(".claude/skills/shared"));
    assert_eq!(tree(&b.home.path().join(".claude/skills/shared")), shared);
    a.has_logged("1 other machines compared").await;

    // Edits on both: one conflict on each, and no file changes.
    put_skill(a.home.path(), ".claude/skills", "shared", "Again on A.");
    put_skill(b.home.path(), ".claude/skills", "shared", "Again on B.");
    let shared_a = tree(&a.home.path().join(".claude/skills/shared"));
    let shared_b = tree(&b.home.path().join(".claude/skills/shared"));
    let mut a = a.restart().await;
    let differs = "Skill shared differs on box-a and box-b; nothing changed";
    a.has_logged(differs).await;
    b.has_logged(differs).await;
    assert_eq!(tree(&a.home.path().join(".claude/skills/shared")), shared_a);
    assert_eq!(tree(&b.home.path().join(".claude/skills/shared")), shared_b);
    while let Ok(line) = b.lines.try_recv() {
        b.log.push(line);
    }
    for node in [&a, &b] {
        let told = node.log.iter().filter(|l| l.contains(differs)).count();
        assert_eq!(told, 1, "{:#?}", node.log);
    }
    let updates = b
        .log
        .iter()
        .filter(|l| l.contains("updated shared"))
        .count();
    assert_eq!(updates, 1, "B took only the first edit");
}
