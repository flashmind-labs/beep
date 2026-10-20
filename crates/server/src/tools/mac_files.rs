//! Lists, finds and reads files on the user's Mac, through the Mac app the
//! run started on. A read file is saved in the file folder too, so code can
//! open it and `send_file` can pass it on.

use super::device_result;
use crate::{
    app::App,
    attachments::{self, Handed},
    protocol::{DeviceAnswer, DeviceQuery, MacFileCommand},
    sandbox::Sandbox,
    user::User,
};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) const NAME: &str = "mac_files";

pub(crate) struct MacFilesTool {
    pub(crate) app: Arc<App>,
    pub(crate) run_id: String,
    pub(crate) user: Arc<User>,
    pub(crate) sandbox: Option<Arc<Sandbox>>,
}

/// What the Mac app answers a read with.
#[derive(Deserialize)]
struct ReadFile {
    name: String,
    mime: String,
    data: String,
}

#[async_trait]
impl Tool for MacFilesTool {
    fn name(&self) -> &str {
        NAME
    }
    // Answered by the Mac app, one request at a time.
    fn concurrent(&self) -> bool {
        false
    }
    fn description(&self) -> &str {
        "Use files on the user's Mac, the computer they are writing from, anywhere in their home folder. \
         `list` shows a folder's files (path such as `~/Desktop` or `~/Downloads`); `find` searches by part of a \
         file name under `folder` (default `~`); `read` opens one file: text comes back inline, and every file \
         is also saved in the file folder, so execute_code can open a PDF or spreadsheet and send_file can send it \
         to the user's phone. Files up to 5 MB. When the user names a file loosely, find it before asking."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["list", "find", "read"] },
                "path": { "type": "string", "description": "Folder to list or file to read, such as ~/Desktop/report.pdf" },
                "name": { "type": "string", "description": "Part of the file name to find (for find)" },
                "folder": { "type": "string", "description": "Folder to search under (for find), default ~" }
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }
    fn timeout_secs(&self) -> Option<u64> {
        Some(60)
    }
    fn humanize(&self, args: &Value) -> String {
        let arg = |key| args.get(key).and_then(Value::as_str).unwrap_or("");
        let last = |path: &str| {
            path.trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or(path)
                .to_owned()
        };
        match arg("action") {
            "list" => format!("Looking in {}", last(arg("path"))),
            "find" => format!("Finding \u{201c}{}\u{201d}", arg("name")),
            _ => format!("Reading {}", last(arg("path"))),
        }
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let arg = |key| {
            ctx.args
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
        };
        let command = match ctx.args.get("action").and_then(Value::as_str) {
            Some("list") => MacFileCommand::List {
                path: arg("path").unwrap_or_else(|| "~".into()),
            },
            Some("find") => match arg("name") {
                Some(name) => MacFileCommand::Find {
                    name,
                    folder: arg("folder").unwrap_or_else(|| "~".into()),
                },
                None => {
                    return Ok(ToolResult::failure(
                        ctx.tool_call_id,
                        "name is required for find",
                    ));
                }
            },
            Some("read") => match arg("path") {
                Some(path) => MacFileCommand::Read { path },
                None => {
                    return Ok(ToolResult::failure(
                        ctx.tool_call_id,
                        "path is required for read",
                    ));
                }
            },
            _ => {
                return Ok(ToolResult::failure(
                    ctx.tool_call_id,
                    "action must be list, find or read",
                ));
            }
        };
        let reading = matches!(command, MacFileCommand::Read { .. });
        let answer = self
            .app
            .device_query(&self.run_id, DeviceQuery::MacFiles { command }, 45)
            .await;
        let output = match (reading, answer) {
            (true, Ok(DeviceAnswer::Ok { output })) => output,
            (_, answer) => return Ok(device_result(ctx.tool_call_id, answer)),
        };
        let file: ReadFile = match serde_json::from_str(&output) {
            Ok(file) => file,
            Err(e) => {
                return Ok(ToolResult::failure(
                    ctx.tool_call_id,
                    format!("The Mac app answered something unreadable: {e}"),
                ));
            }
        };
        let Ok(data) = STANDARD.decode(file.data.as_bytes()) else {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                "The Mac app sent a file that could not be decoded",
            ));
        };
        let handed = Handed {
            name: &file.name,
            mime: &file.mime,
            data: &data,
        };
        let path =
            attachments::save(self.sandbox.as_deref(), self.user.external_id(), &handed).await;
        Ok(ToolResult::success(
            ctx.tool_call_id,
            attachments::describe(&handed, path.as_deref()),
        ))
    }
}
