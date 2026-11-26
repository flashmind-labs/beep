//! The Beep MCP server, whose tools let this node's coding agents work across
//! the user's machines: `start` serves it,
//! and `Launch` names it in each session's `session/new` as
//! `beep-node mcp --session <id>`.

use beep_proto::SessionId;
use crate::{
    agents::{Answered, Asking, socket},
    link::Offers,
    run::local_socket::{self, Listener},
};
use agent_client_protocol::v1::schema::{EnvVariable, McpServer, McpServerStdio};
use anyhow::{Context, Result, bail};
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

mod catalog;
pub(crate) mod children;
pub(crate) mod configured;
mod debug;
mod desktop;
mod devices;
pub(crate) mod harness;
mod machines;
pub(crate) mod peer_tools;
mod media;
pub(in crate::agents) mod review;
mod server;
mod team;
mod transcripts;


pub use super::calls::{SERVER_NAME, command_name};
pub(crate) use desktop::answer as answer_projects;
pub use super::desktop_call::ProjectTools;
pub(crate) use team::{answer as answer_team, deliver_handoffs};
pub(crate) use server::Backend;
use server::Called;

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

    fn token(&self, session: &SessionId) -> blake3::Hash {
        blake3::keyed_hash(&self.key, session.as_ref().as_bytes())
    }

    /// Whether `token` is the one `session` was given. The comparison
    /// takes the same time however much matches.
    fn verify(&self, session: &SessionId, token: &str) -> bool {
        blake3::Hash::from_hex(token.trim()).is_ok_and(|given| given == self.token(session))
    }

    /// The entry `session/new` passes for `session`.
    pub(crate) fn server(&self, session: &SessionId) -> McpServer {
        let args = Invocation {
            dir: self.dir.clone(),
            session: session.clone(),
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
    pub session: SessionId,
}

impl Invocation {
    fn args(&self) -> Vec<String> {
        vec![
            "--dir".to_owned(),
            self.dir.display().to_string(),
            "mcp".to_owned(),
            "--session".to_owned(),
            self.session.to_string(),
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

        let session = session.into_string().ok().filter(|id| !id.is_empty()).map(Into::<SessionId>::into)?;
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

/// What `server` uses on a node: the mesh for calls, the sessions for
/// questions.
struct Node {
    offers: Offers,
    /// The devices the agents drive, attached while in use.
    screens: devices::Screens,
}

impl Backend for Node {
    async fn call(&self, request: &Request) -> Result<Reply> {
        match request.tool {
            Tool::ListProjects => {
                return Ok(crate::agents::projects::call(
                    self.offers.agents.mesh().as_ref(),
                    &request.args,
                ));
            }
            Tool::HarnessInfo => return Ok(harness::call(&self.offers, request).await),
            Tool::Projects => return Ok(desktop::call(&self.offers, request).await),
            Tool::Review => return Ok(review::review(&self.offers, request).await),
            Tool::SubmitReview => return Ok(review::submit(&self.offers, request)),
            Tool::StartAgent
            | Tool::AgentStatus
            | Tool::SendToAgent
            | Tool::StopAgent
            | Tool::ListAgents => return Ok(children::call(&self.offers, request).await),
            Tool::Team | Tool::TeamAsk => return Ok(team::call(&self.offers, request).await),
            Tool::TeamJoin => return Ok(team::join_call(&self.offers, request).await),
            Tool::BeepStatus | Tool::BeepLogs | Tool::BeepMetrics | Tool::FileReport => {
                return Ok(debug::call(&self.offers, request).await);
            }
            _ => {}
        }

        // The machines of the mesh answer for themselves.
        let Some(mesh) = self.offers.mesh.get() else {
            return Ok(Reply::Done {
                text: NO_MESH.to_owned(),
                failed: true,
            });
        };

        if let Some(refused) = machines::run_here(&mesh, request) {
            return Ok(refused);
        }

        Ok(machines::call(&self.offers, &mesh, request).await)
    }

    async fn device(&self, session: &SessionId, args: Value) -> Called {
        match self.offers.mesh.get() {
            Some(mesh) => devices::call(&self.offers, &mesh, &self.screens, session, args).await,
            None => Called::failed(NO_MESH),
        }
    }

    async fn read_transcript(&self, args: serde_json::Value) -> Result<Reply> {
        transcripts::call(&self.offers, args).await
    }

    async fn ask(&self, session: &SessionId, question: Asking) -> Result<Answered> {
        self.offers.agents.ask(session, question).await
    }

    fn reviewer(&self, session: &SessionId) -> bool {
        self.offers.agents.is_reviewer(session)
    }

    fn assistant(&self, session: &SessionId) -> bool {
        self.offers.agents.is_assistant(session)
    }

    fn team(&self, session: &SessionId) -> bool {
        team::team_of(&self.offers, session).is_some()
    }

    fn alone(&self) -> bool {
        self.offers
            .mesh
            .get()
            .is_none_or(|mesh| crate::agents::machine_list::alone(&mesh))
    }

    fn media(&self, session: &SessionId) -> Option<media::Media> {
        self.offers.agents.folder_of(session).map(media::Media::new)
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
pub async fn listen(launch: Arc<Launch>, offers: Offers) -> Result<()> {
    let socket = &launch.socket;
    socket::prepare(socket, &launch.dir)?;
    let mut listener = Listener::bind(socket)?;
    log::info!(
        "serving the Beep MCP server on {}",
        local_socket::shown(socket)
    );
    offers.agents.serve_mcp(launch.clone());
    offers.agents.serve_mesh(offers.mesh.clone());
    let node = Arc::new(Node {
        offers,
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
#[path = "../../tests/agents_mcp.rs"]
mod tests;
