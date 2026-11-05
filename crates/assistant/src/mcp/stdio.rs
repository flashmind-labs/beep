//! A server run as a child process of this Mac, speaking one JSON message per line on its
//! standard input and output. One request is in flight at a time: it holds the pipes from the
//! write to its answer, and a late answer to an earlier, timed-out request is skipped by its id.

use super::{McpError, config::EnvVar};
use crate::AsyncMutex;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use std::{
    process::Stdio as Pipe,
    sync::{Mutex, PoisonError},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};

/// Longest line read from the server.
const MAX_LINE: u64 = 4 * 1024 * 1024;
/// Lines that are not JSON messages, such as a banner, put up with before giving up.
const MAX_NOISE: usize = 200;

pub(super) struct Stdio {
    io: AsyncMutex<Io>,
    /// Kept to end the process; `kill_on_drop` ends it too when this is dropped.
    child: Mutex<Child>,
}

struct Io {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    last_id: u64,
}

impl Stdio {
    pub(super) fn start(command: &str, args: &[String], env: &[EnvVar]) -> Result<Self, McpError> {
        let mut process = Command::new(command);
        process
            .args(args)
            .stdin(Pipe::piped())
            .stdout(Pipe::piped())
            // Nobody reads it, and a full pipe would stop the server.
            .stderr(Pipe::null())
            .kill_on_drop(true);
        for var in env {
            process.env(&var.name, var.value.expose_secret());
        }
        let mut child = process.spawn().map_err(|e| McpError::Start {
            command: command.to_owned(),
            reason: e.to_string(),
        })?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(McpError::Closed);
        };
        Ok(Self {
            io: AsyncMutex::new(Io {
                stdin,
                stdout: BufReader::new(stdout),
                last_id: 0,
            }),
            child: Mutex::new(child),
        })
    }

    pub(super) async fn request(&self, method: &str, params: Value) -> Result<Value, McpError> {
        let mut io = self.io.lock().await;
        io.last_id += 1;
        let id = io.last_id;
        io.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await?;
        let mut noise = 0;
        loop {
            let Some(message) = io.read().await? else {
                noise += 1;
                if noise > MAX_NOISE {
                    return Err(McpError::Protocol(
                        "too many lines that are not JSON".into(),
                    ));
                }
                continue;
            };
            let answer_id = message.get("id");
            match (answer_id, message.get("method")) {
                // The server asks something of us (ping, roots, sampling): none is supported.
                (Some(asked), Some(_)) => {
                    let refusal = json!({
                        "jsonrpc": "2.0",
                        "id": asked,
                        "error": {"code": -32601, "message": "Method not found"},
                    });
                    io.send(&refusal).await?;
                }
                (Some(answered), None) if *answered == json!(id) => return super::answer(message),
                // A notification, or the late answer to a request that timed out.
                _ => {}
            }
        }
    }

    pub(super) async fn notify(&self, method: &str, params: Value) -> Result<(), McpError> {
        let mut io = self.io.lock().await;
        io.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
            .await
    }

    /// Ends the process now.
    pub(super) fn kill(&self) {
        let mut child = self.child.lock().unwrap_or_else(PoisonError::into_inner);
        // It may have ended already, which is what this wants.
        child.start_kill().ok();
    }
}

impl Io {
    async fn send(&mut self, message: &Value) -> Result<(), McpError> {
        let mut line = message.to_string();
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.flush().await?;
        Ok(())
    }

    /// The next line as a JSON message; `None` for a line that is not one.
    async fn read(&mut self) -> Result<Option<Value>, McpError> {
        let mut line = String::new();
        let n = (&mut self.stdout)
            .take(MAX_LINE)
            .read_line(&mut line)
            .await?;
        if n == 0 {
            return Err(McpError::Closed);
        }
        if !line.ends_with('\n') && u64::try_from(n).is_ok_and(|n| n >= MAX_LINE) {
            return Err(McpError::TooLong);
        }
        Ok(serde_json::from_str::<Value>(line.trim())
            .ok()
            .filter(Value::is_object))
    }
}
