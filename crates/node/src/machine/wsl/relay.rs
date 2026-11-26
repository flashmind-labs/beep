//! A coding agent inside WSL, spoken to from Windows: the node starts its ACP
//! program in the distribution and relays its stdio, turning
//! Windows paths into the distribution's (`C:\code` as `/mnt/c/code`) on the
//! way in and back on the way out. Only the values of the keys that hold
//! paths change, never text.

use super::distro::{linux_path, windows_drive_path};
use crate::process::line_reader::{Line, LineReader, MAX_MESSAGE};
use anyhow::Result;
use serde_json::Value;
use std::path::Path;
use tokio::io::{AsyncBufRead, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio_util::task::AbortOnDropHandle;

/// Which way a message crosses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Toward {
    /// From the node to the agent in WSL.
    Linux,
    /// From the agent back to the node.
    Windows,
}

/// Keys whose string value is a path.
const PATH_KEYS: &[&str] = &["cwd", "path", "file_path", "notebook_path", "abs_path", "filePath"];
/// Keys whose value is a list of paths.
const PATH_LISTS: &[&str] = &["additionalDirectories", "additional_directories"];

/// Relays ACP between the session (`client_in`, `client_out`) and the agent
/// in WSL (`agent_in`, `agent_out`) until the agent stops writing.
pub(crate) async fn relay(
    client_in: impl AsyncRead + Unpin + Send + 'static,
    client_out: impl AsyncWrite + Unpin + Send,
    agent_in: impl AsyncWrite + Unpin + Send + 'static,
    agent_out: impl AsyncRead + Unpin + Send,
) -> Result<()> {
    // The client closing its side closes the agent's stdin; the relay ends
    // once the agent stops writing, so its last answers still arrive.
    let inbound = AbortOnDropHandle::new(tokio::spawn(pump(
        BufReader::new(client_in),
        agent_in,
        Toward::Linux,
    )));
    let outbound = pump(BufReader::new(agent_out), client_out, Toward::Windows).await;
    drop(inbound);

    outbound
}

/// Copies lines from `from` to `to`, each JSON one with its paths turned
/// `toward` the other side.
async fn pump(
    from: impl AsyncBufRead + Unpin + Send,
    mut to: impl AsyncWrite + Unpin + Send,
    toward: Toward,
) -> Result<()> {
    let mut lines = LineReader::new(from, MAX_MESSAGE);

    while let Some(line) = lines.next().await? {
        let Line::Text(text) = line else {
            log::warn!("dropped a line over {MAX_MESSAGE} bytes between Windows and WSL");
            continue;
        };
        let text = match serde_json::from_str::<Value>(&text) {
            Ok(mut message) => {
                translate(&mut message, toward);
                message.to_string()
            }
            Err(_) => text,
        };
        to.write_all(text.as_bytes()).await?;
        to.write_all(b"\n").await?;
        to.flush().await?;
    }

    Ok(())
}

/// `value` with the paths in it turned `toward` the other side: the values
/// of `PATH_KEYS` and `PATH_LISTS`, `file://` URIs, and a stdio MCP
/// server's command, which runs as a Windows program from WSL.
pub(crate) fn translate(value: &mut Value, toward: Toward) {
    match value {
        Value::Object(fields) => {
            for (key, field) in fields.iter_mut() {
                match field {
                    Value::String(text) if PATH_KEYS.contains(&key.as_str()) => turn(text, toward),
                    Value::String(text) if key == "uri" => turn_uri(text, toward),
                    Value::Array(items) if PATH_LISTS.contains(&key.as_str()) => {
                        for item in items.iter_mut() {
                            if let Value::String(text) = item {
                                turn(text, toward);
                            }
                        }
                    }
                    Value::Array(servers) if key == "mcpServers" && toward == Toward::Linux => {
                        servers.iter_mut().for_each(mcp_server);
                    }
                    other => translate(other, toward),
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| translate(item, toward)),
        _ => {}
    }
}

/// A stdio MCP server, which Claude Code or another agent starts from WSL:
/// its Windows program by its path in the distribution, and its variables
/// named in `WSLENV` so they reach it.
fn mcp_server(server: &mut Value) {
    let Some(Value::String(command)) = server.get_mut("command") else {
        return;
    };
    turn(command, Toward::Linux);

    let Some(Value::Array(env)) = server.get_mut("env") else {
        return;
    };
    let names: Vec<String> = env
        .iter()
        .filter_map(|var| var["name"].as_str())
        .map(str::to_owned)
        .collect();
    env.push(serde_json::json!({ "name": "WSLENV", "value": names.join(":") }));
}

/// `text`, a path, turned `toward` the other side when it is on a drive.
pub(crate) fn turn(text: &mut String, toward: Toward) {
    let turned = match toward {
        Toward::Linux => linux_path(Path::new(text.as_str())).map(|path| path.to_string()),
        Toward::Windows => windows_drive_path(text),
    };

    if let Some(turned) = turned {
        *text = turned;
    }
}

/// A `file://` URI turned `toward` the other side: `file:///C:/code` and
/// `file:///mnt/c/code`.
fn turn_uri(text: &mut String, toward: Toward) {
    let Some(path) = text.strip_prefix("file://") else {
        return;
    };
    let mut path = match toward {
        Toward::Linux => path.trim_start_matches('/').to_owned(),
        Toward::Windows => path.to_owned(),
    };
    let before = path.clone();
    turn(&mut path, toward);

    if path != before {
        let path = match toward {
            Toward::Linux => path,
            Toward::Windows => format!("/{}", path.replace('\\', "/")),
        };
        *text = format!("file://{path}");
    }
}

#[cfg(test)]
#[path = "../../tests/machine_wsl_relay.rs"]
mod tests;
