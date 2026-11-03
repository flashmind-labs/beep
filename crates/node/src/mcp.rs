//! The Beep MCP server: tools that let the coding agents this node runs
//! work across the user's machines.
//!
//! Each session's `session/new` (and `session/load`) names the server as a
//! stdio command, `beep-node mcp --session <id>`, with a token for that
//! session in its environment. The command only pipes its stdin and stdout
//! to the running node over the local socket `mcp.sock` in the node's
//! folder (`pipe`), which only this user can open; the node checks the
//! token and serves MCP on the connection (`server`). `ask_user` and
//! approvals become questions in the session, as the agent's own are; every
//! other call goes to the server over `tools/1`, which runs it on the
//! machine it names.

use crate::{
    agents::{Agents, Answered, Asking},
    local_socket::{self, Listener},
};
use agent_client_protocol::schema::v1::{EnvVariable, McpServer, McpServerStdio};
use anyhow::{Context, Result, bail};
use beep_proto::node::{
    Open, Opened, link,
    tools::{PROTOCOL, Reply, Request},
};
use iroh::endpoint::Connection;
use serde::Deserialize;
use std::{
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
use uuid::Uuid;

mod catalog;
mod server;

pub(crate) use server::Backend;

/// The server's name, as agents prefix its tools: `mcp__beep__run_on`.
pub const SERVER_NAME: &str = "beep";
/// The env var the token reaches `beep-node mcp` in.
pub const TOKEN_ENV: &str = "BEEP_MCP_TOKEN";
const SOCKET: &str = "mcp.sock";
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

/// What the agent hears ahead of its first prompt when it has the server.
pub(crate) const BRIEF: &str = "Beep: the `beep` MCP server's tools reach the user's other \
    machines. list_machines says which machines there are, whether each is online, its OS, \
    what it can do (Xcode, Android, GPU, Docker, Blender) and its coding agents: pick the \
    machine whose OS and tools fit the work, such as a macOS machine with Xcode for iOS and \
    Mac builds, or one with a GPU for heavy jobs. run_on runs a command there, or starts a \
    coding agent there with a prompt; job_status and cancel follow what it started. \
    sync_project syncs a project's files to the user's other machines before work there, and \
    send_files copies files to another machine. ask_user asks the user a question and waits \
    for the answer. Running and sending may wait for the user to allow it.";

/// How sessions start the server, and the key their tokens are made with.
pub struct Launch {
    /// The `beep-node` to run.
    exe: PathBuf,
    /// The node's folder, which holds the socket.
    dir: PathBuf,
    /// Made fresh each run: a token is good while this node runs.
    key: [u8; 32],
}

impl Launch {
    /// Sessions run `exe mcp` against the node in `dir`.
    pub fn new(exe: PathBuf, dir: PathBuf) -> Self {
        let mut key = [0; 32];
        key[..16].copy_from_slice(Uuid::new_v4().as_bytes());
        key[16..].copy_from_slice(Uuid::new_v4().as_bytes());
        Self { exe, dir, key }
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
        let args = vec![
            "--dir".to_owned(),
            self.dir.display().to_string(),
            "mcp".to_owned(),
            "--session".to_owned(),
            session.to_owned(),
        ];
        let token = self.token(session).to_hex().to_string();
        McpServer::Stdio(
            McpServerStdio::new(SERVER_NAME, &self.exe)
                .args(args)
                .env(vec![EnvVariable::new(TOKEN_ENV, token)]),
        )
    }
}

/// The socket in the node's folder `dir`.
pub fn socket_path(dir: &Path) -> PathBuf {
    dir.join(SOCKET)
}

/// The server's connection, for the calls it runs: set while the node is
/// connected to a server that takes `tools/1`.
#[derive(Clone)]
pub struct Upstream(Arc<watch::Sender<Option<Connection>>>);

impl Default for Upstream {
    fn default() -> Self {
        Self(Arc::new(watch::Sender::new(None)))
    }
}

impl Upstream {
    pub(crate) fn set(&self, connection: Option<Connection>) {
        self.0.send_replace(connection);
    }

    /// Runs `request` on the server.
    pub(crate) async fn call(&self, request: &Request) -> Result<Reply> {
        let connection = self.0.borrow().clone().context(
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

/// What `server` uses on a node: the server for calls, the sessions for
/// questions.
struct Node {
    agents: Arc<Agents>,
    upstream: Upstream,
}

impl Backend for Node {
    async fn call(&self, request: &Request) -> Result<Reply> {
        self.upstream.call(request).await
    }

    async fn ask(&self, session: &str, question: Asking) -> Result<Answered> {
        self.agents.ask(session, question).await
    }
}

/// The first line a connection sends: whose it is.
#[derive(Deserialize)]
struct Hello {
    session: String,
    token: String,
}

/// Takes MCP connections on the socket in `launch`'s folder until the node
/// stops, and gives sessions the server from now on.
pub async fn listen(launch: Arc<Launch>, agents: Arc<Agents>, upstream: Upstream) -> Result<()> {
    let socket = socket_path(&launch.dir);
    let mut listener = Listener::bind(&socket)?;
    log::info!(
        "serving the Beep MCP server on {}",
        local_socket::shown(&socket)
    );
    agents.serve_mcp(launch.clone());
    let node = Arc::new(Node { agents, upstream });
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
    if !node.agents.runs(&hello.session) {
        bail!(
            "refused an MCP connection: session {} does not run",
            hello.session
        );
    }
    log::debug!("MCP connection for session {}", hello.session);
    server::serve(node.as_ref(), &hello.session, reader, write).await
}

/// `beep-node mcp`: connects to the node in `dir` for `session`, with the
/// token in `TOKEN_ENV`, and pipes stdin and stdout to it until either
/// side ends.
pub async fn pipe(dir: &Path, session: &str) -> Result<()> {
    let token = std::env::var(TOKEN_ENV)
        .with_context(|| format!("{TOKEN_ENV} is not set; the node sets it for its sessions"))?;
    let socket = socket_path(dir);
    let stream = local_socket::connect(&socket)
        .await
        .with_context(|| format!("no beep-node runs with its folder in {}", dir.display()))?;
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
}
