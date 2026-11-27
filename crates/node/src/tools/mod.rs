//! The Beep MCP server, whose tools let this node's coding agents work across
//! the user's machines: `bind` and `serve` start it,
//! and `Launch` names it in each session's `session/new` as
//! `beep-node mcp --session <id>`. `tools` sits above `team` and `agents` and
//! below `link`, which builds its `Node`.

use crate::{
    agents::{
        Answered, Asking,
        mcp_launch::{Launch, SOCKET_ENV, TOKEN_ENV},
        socket,
    },
    run::local_socket::{self, Listener},
};
use anyhow::{Context, Result, bail};
use beep_proto::SessionId;
use beep_proto::node::tools::{Reply, Request, Tool};
use serde::Deserialize;
use serde_json::Value;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    task::JoinSet,
    time::timeout,
};
use tokio_util::task::AbortOnDropHandle;

mod caller;
mod catalog;
pub(crate) mod children;
pub(crate) mod configured;
mod debug;
mod desktop;
pub(crate) mod desktop_call;
mod devices;
pub(crate) mod harness;
mod machines;
mod media;
mod node;
pub(crate) mod peer_tools;
pub(crate) mod review;
mod server;
pub(crate) mod team;
pub(crate) mod team_link;
mod transcripts;

pub use crate::agents::calls::{SERVER_NAME, command_name};
pub use crate::agents::mcp_launch::Invocation;
pub(crate) use caller::Caller;
pub use desktop_call::ProjectTools;
pub use node::Node;
pub(crate) use server::Backend;
use server::Called;

/// The most MCP connections served at once: one per running session, and
/// a node runs at most a few dozen.
const MAX_CONNECTIONS: usize = 64;
/// How long a connection has to say whose it is.
const HELLO_LIMIT: Duration = Duration::from_secs(5);
const MAX_HELLO: u64 = 4096;
impl Invocation {
    /// Pipes stdin and stdout to the node until either side ends.
    pub async fn run(&self) -> Result<()> {
        pipe(&self.dir, &self.session).await
    }
}

/// The socket of the Beep MCP server, bound and waiting for `serve`.
pub struct Socket {
    launch: Arc<Launch>,
    listener: Listener,
}

impl Socket {
    /// How sessions start the server on this socket.
    #[must_use]
    pub fn launch(&self) -> Arc<Launch> {
        self.launch.clone()
    }
}

/// Binds the socket of the Beep MCP server in `dir`, with sessions running
/// this executable as the server's command: `serve` takes it, and sessions
/// start with its `launch`. `None`, logged, when this executable is not found
/// or the socket does not bind.
pub fn bind(dir: &Path) -> Option<Socket> {
    let exe = match std::env::current_exe().and_then(std::path::absolute) {
        Ok(exe) => exe,
        Err(e) => {
            log::warn!("not serving the Beep MCP server: this binary is not found: {e}");
            return None;
        }
    };
    let dir = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    let launch = Arc::new(Launch::new(exe, dir));
    let listener =
        socket::prepare(&launch.socket, &launch.dir).and_then(|()| Listener::bind(&launch.socket));

    match listener {
        Ok(listener) => Some(Socket { launch, listener }),
        Err(e) => {
            log::warn!("not serving the Beep MCP server: {e:#}");
            None
        }
    }
}

/// Serves the Beep MCP server for `node`'s sessions on `socket`, bound by
/// `bind`, until the handle drops.
pub fn serve(socket: Socket, node: Node) -> AbortOnDropHandle<()> {
    AbortOnDropHandle::new(tokio::spawn(async move {
        if let Err(e) = listen(socket, node).await {
            log::warn!("not serving the Beep MCP server: {e:#}");
        }
    }))
}

/// What `server` uses on a node: the mesh for calls, the sessions for
/// questions.
struct Served {
    node: Node,
    /// The devices the agents drive, attached while in use.
    screens: devices::Screens,
}

impl Backend for Served {
    async fn call(&self, request: &Request) -> Result<Reply> {
        match request.tool {
            Tool::ListProjects => {
                return Ok(crate::agents::projects::call(
                    self.node.agents.links.mesh().as_ref(),
                    &request.args,
                ));
            }
            Tool::HarnessInfo => return Ok(harness::call(&self.node, request).await),
            Tool::Projects => return Ok(desktop::call(&self.node, request).await),
            Tool::Review => return Ok(review::review(&self.node, request).await),
            Tool::SubmitReview => return Ok(review::submit(&self.node, request)),
            Tool::StartAgent
            | Tool::AgentStatus
            | Tool::SendToAgent
            | Tool::StopAgent
            | Tool::ListAgents => return Ok(children::call(&self.node, request).await),
            Tool::Team | Tool::TeamAsk => return Ok(team::call(&self.node, request).await),
            Tool::TeamJoin => return Ok(team::join_call(&self.node, request).await),
            Tool::BeepStatus | Tool::BeepLogs | Tool::BeepMetrics | Tool::FileReport => {
                return Ok(debug::call(&self.node, request).await);
            }
            _ => {}
        }

        // The machines of the mesh answer for themselves.
        let Some(mesh) = self.node.mesh.get() else {
            return Ok(Reply::Done {
                text: NO_MESH.to_owned(),
                failed: true,
            });
        };

        if let Some(refused) = machines::run_here(&mesh, request) {
            return Ok(refused);
        }

        Ok(machines::call(&self.node, &mesh, request).await)
    }

    async fn device(&self, session: &SessionId, args: Value) -> Called {
        match self.node.mesh.get() {
            Some(mesh) => devices::call(&self.node, &mesh, &self.screens, session, args).await,
            None => Called::failed(NO_MESH),
        }
    }

    async fn read_transcript(&self, args: serde_json::Value) -> Result<Reply> {
        transcripts::call(&self.node, args).await
    }

    async fn ask(&self, session: &SessionId, question: Asking) -> Result<Answered> {
        self.node.agents.sessions.ask(session, question).await
    }

    fn reviewer(&self, session: &SessionId) -> bool {
        self.node.agents.reviews.is_reviewer(session)
    }

    fn assistant(&self, session: &SessionId) -> bool {
        self.node.agents.reviews.is_assistant(session)
    }

    fn team(&self, session: &SessionId) -> bool {
        team::team_of(&self.node, session).is_some()
    }

    fn alone(&self) -> bool {
        self.node
            .mesh
            .get()
            .is_none_or(|mesh| crate::agents::machine_list::alone(&mesh))
    }

    fn media(&self, session: &SessionId) -> Option<media::Media> {
        self.node
            .agents
            .sessions
            .folder_of(session)
            .map(media::Media::new)
    }
}

/// Why a node outside a mesh reaches no machine.
const NO_MESH: &str = "This machine is not in a mesh yet, so it reaches no other machines. Join \
                       it with `beep-node mesh join <ticket>`.";

/// The first line a connection sends: whose it is.
#[derive(Deserialize)]
struct Hello {
    session: SessionId,
    token: String,
}

/// Takes MCP connections on the socket in `launch`'s folder until the node
/// stops, and gives sessions the server from now on. `beep-node mcp` only
/// pipes its stdio to this socket, which only this user can open; the node
/// checks the session's token and serves MCP itself. `ask_user` becomes a
/// question in the session; every other call goes to the mesh over
/// `tools/1`.
async fn listen(socket: Socket, node: Node) -> Result<()> {
    let Socket {
        launch,
        mut listener,
    } = socket;
    log::info!(
        "serving the Beep MCP server on {}",
        local_socket::shown(&launch.socket)
    );
    let node = Arc::new(Served {
        node,
        screens: devices::Screens::default(),
    });
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
    node: Arc<Served>,
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

    if !node.node.agents.sessions.runs(&hello.session) {
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
pub async fn pipe(dir: &Path, session: &SessionId) -> Result<()> {
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
#[path = "../tests/tools.rs"]
mod tests;
