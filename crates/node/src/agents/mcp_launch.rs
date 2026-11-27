//! How a session starts the Beep MCP server: `Launch` names it in each
//! session's `session/new` as `beep-node mcp --session <id>`, and
//! `Invocation` is that command.

use crate::{calls::SERVER_NAME, socket};
use agent_client_protocol::v1::schema::{EnvVariable, McpServer, McpServerStdio};
use beep_proto::SessionId;
use std::{ffi::OsString, path::PathBuf};

/// The env var the token reaches `beep-node mcp` in.
pub const TOKEN_ENV: &str = "BEEP_MCP_TOKEN";
/// The env var the socket the node bound reaches `beep-node mcp` in.
pub const SOCKET_ENV: &str = "BEEP_MCP_SOCKET";
/// How sessions start the server, and the key their tokens are made with.
pub struct Launch {
    /// The `beep-node` to run.
    exe: PathBuf,
    /// The node's folder.
    pub(crate) dir: PathBuf,
    /// The socket the node listens on, handed to sessions as it is.
    pub(crate) socket: PathBuf,
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

    pub(crate) fn token(&self, session: &SessionId) -> blake3::Hash {
        blake3::keyed_hash(&self.key, session.as_ref().as_bytes())
    }

    /// Whether `token` is the one `session` was given. The comparison
    /// takes the same time however much matches.
    pub(crate) fn verify(&self, session: &SessionId, token: &str) -> bool {
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

        let session = session
            .into_string()
            .ok()
            .filter(|id| !id.is_empty())
            .map(Into::<SessionId>::into)?;
        Some(Self {
            dir: PathBuf::from(dir),
            session,
        })
    }
}
