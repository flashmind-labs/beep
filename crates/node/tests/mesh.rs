//! Real `beep-node` processes with no server, throwaway state folders and
//! relays off. The test process plays the desktop: it creates the mesh,
//! mints tickets and calls the nodes over `beep-peer/1`, the way the real
//! desktop does. A node cannot create a mesh, so none of these tests could
//! start one in a node.
//!
//! Waits ride on the nodes' log lines, the mesh's `updates()` and channels,
//! each under a generous outer timeout; nothing sleeps.

use beep_mesh::{DeviceKind, JOIN_ALPN, JoinTicket, Mesh, PEER_ALPN};
use beep_proto::node::{
    Down, LinkRx, LinkTx, NODE_ALPN, NodeHello, Open, Opened,
    agent::{Event as AgentEvent, Request as AgentRequest},
    link,
    process::{Reply as ProcessReply, Request as ProcessRequest},
    shell::{Event as ShellEvent, PROTOCOL as SHELL, Request as ShellRequest},
    status::{PROTOCOL as STATUS, Report},
    tools::{PROTOCOL as TOOLS, Reply, Request as ToolRequest, Tool},
};
use iroh::{
    Endpoint, EndpointId, SecretKey,
    endpoint::{Connection, presets},
    protocol::{AcceptError, ProtocolHandler, Router},
};
use iroh_tickets::Ticket;
use serde_json::json;
use std::{fs, path::Path, process::Stdio, time::Duration};
use tempfile::TempDir;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
    sync::mpsc,
    time::timeout,
};

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

    /// Waits until `id` is online here, then returns the connection to it.
    async fn reach(&self, id: EndpointId) -> Connection {
        let mut updates = self.mesh.updates();
        timeout(LIMIT, async {
            loop {
                updates.borrow_and_update();
                if self
                    .mesh
                    .devices()
                    .iter()
                    .any(|d| d.device.id == id && d.online)
                    && let Some(conn) = self.mesh.peer(id)
                {
                    return conn;
                }
                updates.changed().await.expect("the mesh is alive");
            }
        })
        .await
        .expect("the node came online in time")
    }
}

/// A `beep-node run` process: its folder, its fake home and its log lines.
struct Node {
    /// Kills the process when dropped.
    _child: Child,
    id: EndpointId,
    dir: TempDir,
    _home: TempDir,
    lines: mpsc::UnboundedReceiver<String>,
    /// Every line logged so far.
    log: Vec<String>,
}

#[derive(Default)]
struct Config {
    server: Option<EndpointId>,
    agent: Option<(String, Vec<String>)>,
}

impl Node {
    async fn start(config: Config) -> Self {
        let dir = TempDir::new().expect("a temp folder");
        let home = TempDir::new().expect("a temp folder");
        let key = SecretKey::generate();
        let key_file = dir.path().join("key");
        fs::write(&key_file, key.to_bytes()).expect("the key is written");
        restrict(&key_file);
        // A random port, so no node here takes the live service's.
        let mut settings = json!({ "name": "box-a", "port": 0 });
        if let Some(server) = config.server {
            settings["server"] = json!(server.to_string());
        }
        if let Some((program, args)) = config.agent {
            settings["agents"] = json!([{ "name": "codex", "program": program, "args": args }]);
        }
        let text = toml::to_string(&toml::Value::try_from(settings).expect("settings are toml"))
            .expect("settings encode");
        fs::write(dir.path().join("node.toml"), text).expect("settings are written");
        let mut child = Command::new(env!("CARGO_BIN_EXE_beep-node"))
            .arg("--dir")
            .arg(dir.path())
            .arg("run")
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .env("XDG_DATA_HOME", home.path().join("data"))
            .env("BEEP_NODE_NO_RELAY", "1")
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
            id: key.public(),
            dir,
            _home: home,
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
        .unwrap_or_else(|_| panic!("the node did not log {text:?} in time"))
    }

    /// `beep-node mesh join -`, with the ticket on stdin.
    async fn join(&self, ticket: &JoinTicket) {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_beep-node"))
            .args(["mesh", "join", "-", "--dir"])
            .arg(self.dir.path())
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
async fn open(conn: &Connection, protocol: &str) -> (Stream, Opened) {
    let (send, recv) = conn.open_bi().await.expect("a stream opens");
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

async fn ready(conn: &Connection, protocol: &str) -> Stream {
    match open(conn, protocol).await {
        (stream, Opened::Ready {}) => stream,
        (_, other) => panic!("{protocol} was not opened: {other:?}"),
    }
}

async fn tool(conn: &Connection, tool: Tool, args: serde_json::Value) -> Reply {
    let mut stream = ready(conn, TOOLS).await;
    stream
        .tx
        .send(&ToolRequest {
            session: "a3f9c1".to_owned(),
            tool,
            args: args.to_string(),
            approved: Default::default(),
        })
        .await
        .expect("the call is sent");
    timeout(LIMIT, stream.rx.recv::<Reply>())
        .await
        .expect("the call answered in time")
        .expect("the reply reads")
        .expect("the node replied")
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

/// The job id a background `run_on` reports.
fn job_id(text: &str) -> String {
    let rest = text.split("Job id: ").nth(1).expect("a job id is named");
    rest.chars().take_while(char::is_ascii_digit).collect()
}

async fn run_background(conn: &Connection, command: &str) -> Reply {
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
