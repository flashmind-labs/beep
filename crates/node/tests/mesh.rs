//! Real `beep-node` processes with no server, throwaway state folders and
//! relays off. The test process plays the desktop: it creates the mesh,
//! mints tickets and calls the nodes over `beep-peer/1`, the way the real
//! desktop does. A node cannot create a mesh, so none of these tests could
//! start one in a node.
//!
//! Waits ride on the nodes' log lines, the mesh's `updates()` and channels,
//! each under a generous outer timeout; nothing sleeps.

use beep_mesh::{DeviceKind, JOIN_ALPN, JoinTicket, Key, Mesh, PEER_ALPN};
use beep_proto::node::{
    Down, LinkRx, LinkTx, NODE_ALPN, NODE_ALPN_NAMED, NodeHello, Open, Opened,
    agent::{
        Event as AgentEvent, MovePhase, Request as AgentRequest, SESSIONS_PROTOCOL, SessionsReply,
        SessionsRequest, Status,
    },
    link,
    process::{Reply as ProcessReply, Request as ProcessRequest},
    shell::{Event as ShellEvent, PROTOCOL as SHELL, Request as ShellRequest},
    status::{PROTOCOL as STATUS, Report},
    tools::{PROTOCOL as TOOLS, Reply, Request as ToolRequest, Tool},
};
use beep_proto::{
    SecretText,
    node::{
        EnvSecret, MachineCredentials,
        harness::{AgentSettings, HarnessConfig},
        mesh_state::{Credential, SessionMove, StateValue, SyncSpec},
        sync::{
            PROTOCOL as SYNC, Project as SyncProject, Reply as SyncReply, Request as SyncRequest,
        },
    },
};
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
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::mpsc,
    time::timeout,
};
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};

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

/// A node that was stopped: its folder and fake home are kept.
struct Stopped {
    dir: TempDir,
    home: TempDir,
    id: EndpointId,
}

impl Stopped {
    async fn start(self) -> Node {
        Node::run(self.dir, self.home, self.id).await
    }
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
}

#[derive(Default)]
struct Config {
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
        Self::run(dir, home, key.public()).await
    }

    /// Stops the process; `Stopped::start` runs it again on the same folder.
    async fn stop(self) -> Stopped {
        let Self {
            mut _child,
            id,
            dir,
            home,
            ..
        } = self;
        _child.kill().await.expect("the node stops");
        Stopped { dir, home, id }
    }

    /// Stops the process and starts it again on the same folder.
    async fn restart(self) -> Self {
        let Self {
            mut _child,
            id,
            dir,
            home,
            ..
        } = self;
        _child.kill().await.expect("the node stops");
        Self::run(dir, home, id).await
    }

    async fn run(dir: TempDir, home: TempDir, id: EndpointId) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_beep-node"))
            .arg("--dir")
            .arg(dir.path())
            .arg("run")
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .env("XDG_DATA_HOME", home.path().join("data"))
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

/// `a` and `b` run a fake coding agent, the script given; each logs to
/// `sent.jsonl` and `sent-b.jsonl` in `work`.
async fn synced_pair_with(agent: Option<&str>, agent_b: Option<&str>) -> Pair {
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
        ..Config::default()
    })
    .await;
    let mut b = Node::start(Config {
        name: Some("box-b"),
        files: vec![roots[1].path().to_owned()],
        agent: agent_b.map(|script| adapter_of(script, &work.path().join("sent-b.jsonl"))),
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
    // Outside what it shares, the node sends nothing.
    let (text, failed) = mcp
        .call(
            "send_files",
            json!({ "machine": "box-b", "paths": ["/etc/hostname"] }),
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
    let record = SessionMove {
        agent: "codex".into(),
        id: MOVED_ID.to_owned(),
        from,
        to,
        project: "proj".to_owned(),
        path: String::new(),
        mode: None,
        stop,
        at: 1_793_696_400_000,
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
