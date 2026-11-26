//! Codex's `app-server`: JSON-RPC over JSON lines on its stdio, without the
//! `jsonrpc` field. A call waits for its answer; what Codex sends meanwhile
//! waits, in order, for the bridge.

use crate::agents::{
    launch::Cli,
    pipe::{Frame, Pipe},
};
use crate::machine::wsl::DistroName;
use crate::process::line_reader::MAX_MESSAGE;
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::{collections::VecDeque, path::Path};

/// Codex wrote a line too long to read while a call waited, most likely
/// its answer, which is then lost.
#[derive(Debug, thiserror::Error)]
#[error("Codex's answer to {method} was over {MAX_MESSAGE} bytes")]
pub(super) struct TooLong {
    method: String,
}

/// What Codex sends of its own accord.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Incoming {
    Notification { method: String, params: Value },
    /// A request Codex waits on, answered by `id`.
    Request { id: Value, method: String, params: Value },
}

/// A running app-server. Dropping it kills it.
pub(super) struct AppServer {
    pipe: Pipe,
    next: u64,
    backlog: VecDeque<Incoming>,
}

impl AppServer {
    /// Starts it on `cli` in `cwd`, inside `wsl` when given, and completes
    /// the `initialize` handshake.
    pub(super) async fn start(cli: &mut Cli, cwd: &Path, wsl: Option<&DistroName>) -> Result<Self> {
        let mut args = cli.args();
        args.push("app-server".to_owned());
        let process = cli.command(wsl, cwd, &args)?;
        let mut server = Self {
            pipe: Pipe::start(cli, process, "Codex", wsl.is_some())?,
            next: 0,
            backlog: VecDeque::new(),
        };

        let client = json!({"name": "beep-node", "title": "Beep", "version": env!("CARGO_PKG_VERSION")});
        // Questions asked mid-turn and collaboration modes are experimental.
        server
            .call("initialize", json!({"clientInfo": client, "capabilities": {"experimentalApi": true}}))
            .await
            .context("Codex did not initialize")?;
        server.pipe.write(&json!({"method": "initialized"}));
        Ok(server)
    }

    /// Calls `method` and waits for its result; Codex's error as an error,
    /// and an error when it exits first.
    pub(super) async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.next += 1;
        let id = self.next;
        self.pipe.write(&json!({"id": id, "method": method, "params": params}));

        loop {
            let frame = match self.pipe.frames.recv().await {
                Some(Frame::Json(frame)) => frame,
                Some(Frame::TooLong) => {
                    return Err(TooLong {
                        method: method.to_owned(),
                    }
                    .into());
                }
                None => anyhow::bail!("Codex exited before it answered {method}"),
            };

            if frame.get("method").is_none() && frame["id"] == id {
                return match frame.get("error") {
                    Some(error) => Err(anyhow!(
                        "{}",
                        error["message"].as_str().unwrap_or("Codex refused")
                    )),
                    None => Ok(frame.get("result").cloned().unwrap_or(Value::Null)),
                };
            }

            self.backlog.extend(incoming(frame));
        }
    }

    /// What Codex sends next; `None` once it exited.
    pub(super) async fn next(&mut self) -> Option<Incoming> {
        if let Some(waiting) = self.backlog.pop_front() {
            return Some(waiting);
        }

        loop {
            if let Frame::Json(frame) = self.pipe.frames.recv().await?
                && let Some(incoming) = incoming(frame)
            {
                return Some(incoming);
            }
        }
    }

    /// Codex sent the notification `method`, which waits to be read.
    pub(super) fn sent(&self, method: &str) -> bool {
        self.backlog
            .iter()
            .any(|incoming| matches!(incoming, Incoming::Notification { method: sent, .. } if sent == method))
    }

    /// Ends Codex, gently first.
    pub(super) async fn stop(self) {
        self.pipe.stop().await;
    }

    /// Answers Codex's request `id`.
    pub(super) fn respond(&self, id: Value, result: Value) {
        self.pipe.write(&json!({"id": id, "result": result}));
    }

    /// Refuses Codex's request `id`.
    pub(super) fn refuse(&self, id: Value, message: &str) {
        self.pipe
            .write(&json!({"id": id, "error": {"code": -32601, "message": message}}));
    }
}

/// A frame Codex sent of its own accord; `None` for an answer, which only a
/// call waits for.
fn incoming(mut frame: Value) -> Option<Incoming> {
    let method = frame.get("method")?.as_str()?.to_owned();
    let params = frame.get_mut("params").map(Value::take).unwrap_or(Value::Null);

    Some(match frame.get_mut("id").map(Value::take) {
        Some(id) => Incoming::Request { id, method, params },
        None => Incoming::Notification { method, params },
    })
}
