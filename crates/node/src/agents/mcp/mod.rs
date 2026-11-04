//! The Beep MCP server, whose tools let this node's coding agents work across
//! the user's machines and share what the user told them: `start` serves it,
//! and `Launch` names it in each session's `session/new` as
//! `beep-node mcp --session <id>`.

use crate::{
    agents::{Answered, Asking},
    link::Offers,
    link::local_socket::{self, Listener},
};
use agent_client_protocol::schema::v1::{EnvVariable, McpServer, McpServerStdio};
use anyhow::{Context, Result, bail};
use beep_proto::node::{
    Open, Opened, link,
    tools::{Approved, PROTOCOL, Reply, Request, Tool},
};
use iroh::endpoint::Connection;
use serde::Deserialize;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    sync::watch,
    task::JoinSet,
    time::timeout,
};
use tokio_util::task::AbortOnDropHandle;

pub(crate) mod calls;
mod catalog;
mod machines;
mod server;
mod socket;

pub(crate) use server::Backend;

/// The server's name, as agents prefix its tools: `mcp__beep__run_on`.
pub const SERVER_NAME: &str = "beep";
/// The env var the token reaches `beep-node mcp` in.
pub const TOKEN_ENV: &str = "BEEP_MCP_TOKEN";
/// The env var the socket the node bound reaches `beep-node mcp` in.
pub const SOCKET_ENV: &str = "BEEP_MCP_SOCKET";
/// The most MCP connections served at once: one per running session, and
/// a node runs at most a few dozen.
const MAX_CONNECTIONS: usize = 64;
/// How long a connection has to say whose it is.
const HELLO_LIMIT: Duration = Duration::from_secs(5);
const MAX_HELLO: u64 = 4096;
/// How long the server has to take a call's stream.
const OPEN_LIMIT: Duration = Duration::from_secs(15);
/// How long one call may run on the server: a command runs at most half
/// an hour, and a sync waits a minute and a half.
const CALL_LIMIT: Duration = Duration::from_secs(35 * 60);
/// How long a starting session waits for the user's standing rules.
const RULES_LIMIT: Duration = Duration::from_secs(3);

/// What the agent hears ahead of its first prompt about the machine tools.
const MACHINES_BRIEF: &str = "Beep: the `beep` MCP server's tools reach the user's other \
    machines. list_machines says which machines there are, whether each is online, its OS, \
    what it can do (Xcode, Android, GPU, Docker, Blender) and its coding agents: pick the \
    machine whose OS and tools fit the work, such as a macOS machine with Xcode for iOS and \
    Mac builds, or one with a GPU for heavy jobs. run_on runs a command there, or starts a \
    coding agent there with a prompt; job_status and cancel follow what it started. \
    sync_project syncs a project's files to the user's other machines before work there, and \
    send_files copies files to another machine. machine_log reads a machine's beep-node log. ask_user asks the user a question and waits \
    for the answer. Running and sending may wait for the user to allow it.";

/// The memory tools, which only the server holds.
const MEMORY_BRIEF: &str = " remember saves a lasting instruction or preference the user \
    states, for every agent on every machine; recall looks up what they said, before a \
    decision it may speak to.";

/// What the agent hears ahead of its first prompt; the memory tools only
/// when `memory`.
pub(crate) fn brief(memory: bool) -> String {
    if memory {
        format!("{MACHINES_BRIEF}{MEMORY_BRIEF}")
    } else {
        MACHINES_BRIEF.to_owned()
    }
}

/// How sessions start the server, and the key their tokens are made with.
pub struct Launch {
    /// The `beep-node` to run.
    exe: PathBuf,
    /// The node's folder.
    dir: PathBuf,
    /// The socket the node listens on, handed to sessions as it is.
    socket: PathBuf,
    /// Made fresh each run: a token is good while this node runs.
    key: [u8; 32],
}

impl Launch {
    /// Sessions run `exe mcp` against the node in `dir`.
    pub fn new(exe: PathBuf, dir: PathBuf) -> Self {
        let socket = socket::path_for(&dir);
        Self {
            exe,
            dir,
            socket,
            key: rand::random(),
        }
    }

    fn token(&self, session: &str) -> blake3::Hash {
        blake3::keyed_hash(&self.key, session.as_bytes())
    }

    /// Whether `token` is the one `session` was given. The comparison
    /// takes the same time however much matches.
    fn verify(&self, session: &str, token: &str) -> bool {
        blake3::Hash::from_hex(token.trim()).is_ok_and(|given| given == self.token(session))
    }

    /// The entry `session/new` passes for `session`.
    pub(crate) fn server(&self, session: &str) -> McpServer {
        let args = Invocation {
            dir: self.dir.clone(),
            session: session.to_owned(),
        }
        .args();
        let token = self.token(session).to_hex().to_string();
        McpServer::Stdio(
            McpServerStdio::new(SERVER_NAME, &self.exe)
                .args(args)
                .env(vec![
                    EnvVariable::new(TOKEN_ENV, token),
                    EnvVariable::new(SOCKET_ENV, self.socket.display().to_string()),
                ]),
        )
    }
}

/// A session's `mcp` command after the binary: `--dir <dir> mcp --session
/// <id>`. `beep-node` reads it with its own parser; the Mac app, whose own
/// executable is the command, reads it with `parse` before it starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// The node's folder, which holds the socket.
    pub dir: PathBuf,
    /// The node's id of the agent's session.
    pub session: String,
}

impl Invocation {
    fn args(&self) -> Vec<String> {
        vec![
            "--dir".to_owned(),
            self.dir.display().to_string(),
            "mcp".to_owned(),
            "--session".to_owned(),
            self.session.clone(),
        ]
    }

    /// The invocation `args` (after the binary) make, when they are
    /// exactly the arguments `Launch` gives a session.
    pub fn parse<I>(args: I) -> Option<Self>
    where
        I: IntoIterator,
        I::Item: Into<OsString>,
    {
        let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
        let [flag, dir, command, session_flag, session] = <[OsString; 5]>::try_from(args).ok()?;
        if flag != "--dir" || command != "mcp" || session_flag != "--session" || dir.is_empty() {
            return None;
        }
        let session = session.into_string().ok().filter(|id| !id.is_empty())?;
        Some(Self {
            dir: PathBuf::from(dir),
            session,
        })
    }

    /// Pipes stdin and stdout to the node until either side ends.
    pub async fn run(&self) -> Result<()> {
        pipe(&self.dir, &self.session).await
    }
}

/// Serves the Beep MCP server for `offers`' sessions from the socket in
/// `dir` until the handle drops, with sessions running this executable as
/// the server's command. `None`, logged, when this executable is not found.
pub fn start(dir: &Path, offers: &Offers) -> Option<AbortOnDropHandle<()>> {
    let exe = match std::env::current_exe().and_then(std::path::absolute) {
        Ok(exe) => exe,
        Err(e) => {
            log::warn!("not serving the Beep MCP server: this binary is not found: {e}");
            return None;
        }
    };
    let dir = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    let launch = Arc::new(Launch::new(exe, dir));
    let offers = offers.clone();
    Some(AbortOnDropHandle::new(tokio::spawn(async move {
        if let Err(e) = listen(launch, offers).await {
            log::warn!("not serving the Beep MCP server: {e:#}");
        }
    })))
}

/// The server's connection, for the calls it runs: set while the node is
/// connected to a server that takes `tools/1`.
#[derive(Clone)]
pub struct Upstream {
    connection: Arc<watch::Sender<Option<Connection>>>,
    /// The node has a server to link to at all.
    has_server: bool,
}

impl Default for Upstream {
    fn default() -> Self {
        Self::new(true)
    }
}

impl Upstream {
    /// For a node that has a server (`server` in `node.toml`) or not.
    pub fn new(has_server: bool) -> Self {
        Self {
            connection: Arc::new(watch::Sender::new(None)),
            has_server,
        }
    }

    pub(crate) fn has_server(&self) -> bool {
        self.has_server
    }

    /// Whether the link to the server is up now.
    pub(crate) fn connected(&self) -> bool {
        self.connection.borrow().is_some()
    }

    pub(crate) fn set(&self, connection: Option<Connection>) {
        self.connection.send_replace(connection);
    }

    /// The user's standing rules for the first prompt of `session`. `None`
    /// when there are none or they cannot be read in time, as from a server
    /// too old to know them: a session starts without them. Their text stays
    /// out of the log.
    pub async fn standing_rules(&self, session: &str) -> Option<String> {
        let request = Request {
            session: session.to_owned(),
            tool: Tool::StandingRules,
            args: "{}".to_owned(),
            approved: Approved::No,
        };
        match timeout(RULES_LIMIT, self.call(&request)).await {
            Ok(Ok(Reply::Done {
                text,
                failed: false,
            })) if !text.is_empty() => Some(text),
            Ok(Ok(_)) => None,
            Ok(Err(e)) => {
                log::debug!("no standing rules for session {session}: {e:#}");
                None
            }
            Err(_) => {
                log::debug!("no standing rules for session {session}: the server took too long");
                None
            }
        }
    }

    /// Runs `request` on the server.
    pub async fn call(&self, request: &Request) -> Result<Reply> {
        let connection = self.connection.borrow().clone().context(
            "this machine is not connected to Beep's server now, or the server is too old \
             for these tools",
        )?;
        let (send, recv) = connection.open_bi().await?;
        let (mut tx, mut rx) = link(send, recv);
        tx.send(&Open {
            protocol: PROTOCOL.to_owned(),
        })
        .await?;
        let opened: Opened = timeout(OPEN_LIMIT, rx.recv())
            .await
            .context("the server did not take the call in time")??
            .context("the server closed the call")?;
        if let Opened::Refused { reason } = opened {
            bail!("the server refused the call: {reason}");
        }
        tx.send(request).await?;
        timeout(CALL_LIMIT, rx.recv())
            .await
            .context("the call ran too long")??
            .context("the server closed the call before answering")
    }
}

/// What `server` uses on a node: the mesh or the server for calls, the
/// sessions for questions.
struct Node {
    offers: Offers,
}

impl Backend for Node {
    async fn call(&self, request: &Request) -> Result<Reply> {
        // In a mesh the machines answer for themselves; the server still
        // takes what is not between machines, and machines the mesh lacks.
        if let Some(mesh) = self.offers.mesh.get() {
            let route = machines::route(
                request.tool,
                self.offers.tools.has_server(),
                self.offers.tools.connected(),
            );
            match route {
                machines::Route::Refuse => {
                    return Ok(Reply::Done {
                        text: "send_files needs the user's approval from the server, which is not \
                               connected now. Try again when it is."
                            .to_owned(),
                        failed: true,
                    });
                }
                machines::Route::Direct => {
                    if let Some(reply) = machines::call(&self.offers, &mesh, request).await {
                        return Ok(reply);
                    }
                }
                machines::Route::Server => {}
            }
        }
        self.offers.tools.call(request).await
    }

    async fn ask(&self, session: &str, question: Asking) -> Result<Answered> {
        self.offers.agents.ask(session, question).await
    }

    /// The memory tools live on the server: a node with none has nothing to
    /// remember with, in a mesh or not, as its brief says.
    fn memory_tools(&self) -> bool {
        self.offers.tools.has_server()
    }
}

/// The first line a connection sends: whose it is.
#[derive(Deserialize)]
struct Hello {
    session: String,
    token: String,
}

/// Takes MCP connections on the socket in `launch`'s folder until the node
/// stops, and gives sessions the server from now on. `beep-node mcp` only
/// pipes its stdio to this socket, which only this user can open; the node
/// checks the session's token and serves MCP itself. `ask_user` and
/// approvals become questions in the session; every other call goes to the
/// server over `tools/1`.
pub async fn listen(launch: Arc<Launch>, offers: Offers) -> Result<()> {
    let socket = &launch.socket;
    socket::prepare(socket, &launch.dir)?;
    let mut listener = Listener::bind(socket)?;
    log::info!(
        "serving the Beep MCP server on {}",
        local_socket::shown(socket)
    );
    offers
        .agents
        .serve_mcp(launch.clone(), offers.tools.clone());
    let node = Arc::new(Node { offers });
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            stream = listener.accept() => {
                let stream = stream?;
                if connections.len() >= MAX_CONNECTIONS {
                    log::warn!("refused an MCP connection: {MAX_CONNECTIONS} already open");
                    continue;
                }
                connections.spawn(connection(launch.clone(), node.clone(), stream));
            }
            Some(ended) = connections.join_next() => match ended {
                Ok(Ok(())) => {}
                Ok(Err(e)) => log::debug!("an MCP connection ended: {e:#}"),
                Err(e) => log::warn!("an MCP connection task failed: {e}"),
            },
        }
    }
}

/// Serves one connection once its hello names a running session with the
/// right token.
async fn connection(
    launch: Arc<Launch>,
    node: Arc<Node>,
    stream: local_socket::Stream,
) -> Result<()> {
    let (read, write) = tokio::io::split(stream);
    let mut reader = BufReader::new(read);
    let mut line = String::new();
    timeout(
        HELLO_LIMIT,
        (&mut reader).take(MAX_HELLO).read_line(&mut line),
    )
    .await
    .context("the connection sent no hello")??;
    // A connection that only checks the socket is there sends nothing.
    if line.trim().is_empty() {
        return Ok(());
    }
    let hello: Hello = serde_json::from_str(&line).context("not an MCP hello")?;
    if !launch.verify(&hello.session, &hello.token) {
        bail!(
            "refused an MCP connection for session {}: wrong token",
            hello.session
        );
    }
    if !node.offers.agents.runs(&hello.session) {
        bail!(
            "refused an MCP connection: session {} does not run",
            hello.session
        );
    }
    log::debug!("MCP connection for session {}", hello.session);
    server::serve(node.as_ref(), &hello.session, reader, write).await
}

/// The socket a session connects to: the one the node named in
/// `SOCKET_ENV`, else (a session from a node before it named one) the
/// node's own choice for `dir`.
fn connect_to(named: Option<OsString>, dir: &Path) -> PathBuf {
    named
        .filter(|named| !named.is_empty())
        .map_or_else(|| socket::path_for(dir), PathBuf::from)
}

/// `beep-node mcp`: connects to the node in `dir` for `session`, with the
/// token in `TOKEN_ENV`, and pipes stdin and stdout to it until either
/// side ends.
pub async fn pipe(dir: &Path, session: &str) -> Result<()> {
    let token = std::env::var(TOKEN_ENV)
        .with_context(|| format!("{TOKEN_ENV} is not set; the node sets it for its sessions"))?;
    let socket = connect_to(std::env::var_os(SOCKET_ENV), dir);
    let stream = local_socket::connect(&socket).await.with_context(|| {
        format!(
            "no beep-node runs with its folder in {} (socket {})",
            dir.display(),
            socket.display()
        )
    })?;
    let (mut from_node, mut to_node) = tokio::io::split(stream);
    let hello = serde_json::json!({ "session": session, "token": token });
    to_node.write_all(format!("{hello}\n").as_bytes()).await?;
    let up = async move {
        let mut stdin = tokio::io::stdin();
        tokio::io::copy(&mut stdin, &mut to_node).await?;
        to_node.shutdown().await?;
        anyhow::Ok(())
    };
    let down = async {
        let mut stdout = tokio::io::stdout();
        tokio::io::copy(&mut from_node, &mut stdout).await?;
        stdout.flush().await?;
        anyhow::Ok(())
    };
    // The agent closing stdin ends it, and so does the node going away.
    tokio::select! {
        result = up => result,
        result = down => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_good_for_its_own_session_only() {
        let launch = Launch::new("/usr/local/bin/beep-node".into(), "/srv/node".into());
        let token = launch.token("a3f9c1").to_hex().to_string();
        assert!(launch.verify("a3f9c1", &token));
        assert!(!launch.verify("b4e0d2", &token));
        assert!(!launch.verify("a3f9c1", "not hex"));
        let other = Launch::new("/usr/local/bin/beep-node".into(), "/srv/node".into());
        assert!(!other.verify("a3f9c1", &token), "a new run has a new key");
        let McpServer::Stdio(server) = launch.server("a3f9c1") else {
            panic!("the server is not a command");
        };
        assert_eq!(server.name, SERVER_NAME);
        assert_eq!(
            server.args,
            ["--dir", "/srv/node", "mcp", "--session", "a3f9c1"]
        );
        assert_eq!(server.env[0].name, TOKEN_ENV);
        assert_eq!(server.env[0].value, token);
    }

    #[test]
    fn a_session_connects_to_the_socket_the_node_bound() {
        let user = "a".repeat(64);
        let dir = PathBuf::from(format!(
            "/Users/{user}/Library/Application Support/com.flashmind.beep.desktop/node"
        ));
        let launch = Launch::new("/srv/Beep.app/Contents/MacOS/beep".into(), dir.clone());
        let McpServer::Stdio(server) = launch.server("a3f9c1") else {
            panic!("the server is not a command");
        };
        let named = server
            .env
            .iter()
            .find(|var| var.name == SOCKET_ENV)
            .expect("the socket is named");
        assert_eq!(Path::new(&named.value), launch.socket);
        assert!(named.value.len() < 104, "{}", named.value);
        assert_eq!(
            connect_to(Some(named.value.clone().into()), &dir),
            launch.socket
        );
        // A session started before the node named its socket.
        assert_eq!(connect_to(None, &dir), launch.socket);
        assert_eq!(
            connect_to(None, Path::new("/srv/node")),
            Path::new("/srv/node/mcp.sock")
        );
    }

    #[test]
    fn a_sessions_arguments_read_back_as_its_invocation() {
        let launch = Launch::new(
            "/srv/Beep.app/Contents/MacOS/beep".into(),
            "/srv/node".into(),
        );
        let McpServer::Stdio(server) = launch.server("a3f9c1") else {
            panic!("the server is not a command");
        };
        assert_eq!(
            Invocation::parse(&server.args),
            Some(Invocation {
                dir: "/srv/node".into(),
                session: "a3f9c1".into(),
            })
        );
    }
}
