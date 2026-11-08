//! Sends the user a file in the chat.

use super::args::text;
use crate::{account::user::User, tools::fetch};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use flashmind::{
    tools::mcp::{McpContent, McpToolCallResult},
    types::tool::{Tool, ToolContext, ToolResult},
};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

pub(crate) const NAME: &str = "send_file";

pub(crate) struct SendFileTool {
    pub(crate) user: Arc<User>,
    pub(crate) session_id: Uuid,
    /// The chat is a research, so its paper can be sent as a PDF.
    pub(crate) paper: bool,
}

/// Where the file comes from, by which arguments were given.
enum Source<'a> {
    /// The research's paper, typeset.
    Paper,
    Url(&'a str),
    Fastmail {
        account: Option<&'a str>,
        email_id: &'a str,
        blob_id: &'a str,
    },
}

/// The bytes to send, with what is known of their name and type.
struct Loaded {
    name: String,
    mime: Option<String>,
    data: Vec<u8>,
}

#[async_trait]
impl Tool for SendFileTool {
    fn name(&self) -> &str {
        NAME
    }
    fn description(&self) -> &str {
        if self.paper {
            return "Send the user a file, such as a PDF, in this chat; it shows as a file they can open or share. For an image to look at, use send_image instead. Use it whenever the user asks you to send, give or share a file with them, rather than emailing it. Give exactly one source: `paper: true` for this research's paper as a typeset PDF, with its sources; `url` for a file at an https URL; or `email_id` with `blob_id` for a Fastmail attachment. Files up to 20 MiB.";
        }
        "Send the user a file, such as a PDF, in this chat; it shows as a file they can open or share. For an image to look at, use send_image instead. Use it whenever the user asks you to send, give or share a file with them, rather than emailing it. Give exactly one source: `url` for a file at an https URL, such as the s3url GMAIL_GET_ATTACHMENT or OUTLOOK_DOWNLOAD_OUTLOOK_ATTACHMENT returns; or `email_id` with `blob_id` for a Fastmail attachment, as FASTMAIL_READ_EMAIL lists it. Files up to 20 MiB."
    }
    fn parameters(&self) -> Value {
        let mut schema = json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "https URL of the file" },
                "email_id": { "type": "string", "description": "Fastmail email id the attachment belongs to" },
                "blob_id": { "type": "string", "description": "The Fastmail attachment's blobId" },
                "account": { "type": "string", "description": "The Fastmail account the email is in, when the FASTMAIL tools take an `account`" },
                "filename": { "type": "string", "description": "Name to show, such as 'BoardingPass.pdf'; taken from the source when left out" }
            },
            "additionalProperties": false
        });
        if self.paper {
            schema["properties"]["paper"] = json!({
                "type": "boolean",
                "description": "true to send this research's paper as a PDF"
            });
        }
        schema
    }
    fn humanize(&self, args: &Value) -> String {
        if args.get("paper").and_then(Value::as_bool) == Some(true) {
            return "Typesetting the paper…".into();
        }
        "Sending file…".into()
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        self.send(ctx).await
    }
}

impl SendFileTool {
    async fn send(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let arg = |key| text(&ctx.args, key);
        let paper = self.paper && ctx.args.get("paper").and_then(Value::as_bool) == Some(true);
        let source = match (arg("url"), arg("email_id"), arg("blob_id")) {
            (None, None, None) if paper => Source::Paper,
            _ if paper => {
                return Ok(ToolResult::failure(
                    ctx.tool_call_id,
                    "Give `paper` alone, or another source without it.",
                ));
            }
            (Some(url), None, None) => Source::Url(url),
            (None, Some(email_id), Some(blob_id)) => Source::Fastmail {
                account: arg("account"),
                email_id,
                blob_id,
            },
            _ => {
                return Ok(ToolResult::failure(
                    ctx.tool_call_id,
                    "Give exactly one source: `url`, or `email_id` with `blob_id`.",
                ));
            }
        };
        let loaded = match self.load(source).await {
            Ok(loaded) => loaded,
            Err(error) => {
                return Ok(ToolResult::failure(
                    ctx.tool_call_id,
                    format!("Could not get the file: {error:#}"),
                ));
            }
        };
        if loaded.data.is_empty() {
            return Ok(ToolResult::failure(ctx.tool_call_id, "The file is empty."));
        }
        let name = arg("filename")
            .map(clean_name)
            .unwrap_or_else(|| clean_name(&loaded.name));
        let mime = loaded
            .mime
            .filter(|mime| mime.contains('/') && mime != "application/octet-stream")
            .unwrap_or_else(|| mime_for(&name, &loaded.data).to_owned());
        let id = self
            .user
            .save_file(self.session_id, &name, &mime, &loaded.data)
            .await?;
        let output = json!({
            "action": NAME,
            "file": { "id": id, "name": name, "mime": mime, "size": loaded.data.len() },
        });
        Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
    }

    async fn load(&self, source: Source<'_>) -> Result<Loaded> {
        match source {
            Source::Paper => {
                let pdf = self
                    .user
                    .research_pdf(self.session_id)
                    .await?
                    .context("the paper is not written yet")?;
                Ok(Loaded {
                    name: pdf.name,
                    mime: Some("application/pdf".to_owned()),
                    data: pdf.data,
                })
            }
            Source::Url(url) => {
                if !url.starts_with("https://") {
                    bail!("only https URLs are allowed");
                }
                let fetched = fetch::fetch_public(url).await?;
                Ok(Loaded {
                    name: fetched.name,
                    mime: fetched
                        .content_type
                        .map(|t| t.split(';').next().unwrap_or("").trim().to_owned()),
                    data: fetched.data,
                })
            }
            Source::Fastmail {
                account,
                email_id,
                blob_id,
            } => {
                let attachment =
                    fastmail_attachment(&self.user, account, email_id, blob_id).await?;
                Ok(Loaded {
                    name: attachment.name.unwrap_or_else(|| "attachment".to_owned()),
                    mime: attachment.mime,
                    data: attachment.data,
                })
            }
        }
    }
}

/// The Fastmail attachment `blob_id` of `email_id` as the file itself, not its text.
pub(crate) async fn fastmail_attachment(
    user: &User,
    account: Option<&str>,
    email_id: &str,
    blob_id: &str,
) -> Result<Attachment> {
    let args = json!({ "emailId": email_id, "blobId": blob_id, "format": "original" });
    let server = user.fastmail_server(account).await?;
    let result = user
        .call_mcp_content(&server, "read_attachment", args)
        .await?;
    let attachment =
        attachment_in(result).context("Fastmail only gives the file itself for PDFs and images")?;
    if attachment.data.len() as u64 > fetch::MAX_DOWNLOAD {
        bail!("the file is larger than {} MiB", fetch::MAX_DOWNLOAD >> 20);
    }
    Ok(attachment)
}

/// A file decoded from an MCP result.
#[derive(Debug, PartialEq)]
pub(crate) struct Attachment {
    pub(crate) name: Option<String>,
    pub(crate) mime: Option<String>,
    pub(crate) data: Vec<u8>,
}

/// The file in Fastmail's `read_attachment` result: a file block, or JSON text.
fn attachment_in(result: McpToolCallResult) -> Option<Attachment> {
    let mut text = String::new();
    for content in result.content {
        match content {
            McpContent::Blob(blob) => {
                return Some(Attachment {
                    name: None,
                    data: STANDARD.decode(&blob.data).ok()?,
                    mime: blob.mime_type,
                });
            }
            McpContent::Text(part) | McpContent::Other(part) => text.push_str(&part),
        }
    }
    let Value::Object(fields) = serde_json::from_str(&text).ok()? else {
        return None;
    };
    let field = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| fields.get(*key).and_then(Value::as_str))
    };
    let data = STANDARD.decode(field(&["data", "blob"])?).ok()?;
    Some(Attachment {
        name: field(&["name"]).map(str::to_owned),
        mime: field(&["mimeType", "type"]).map(str::to_owned),
        data,
    })
}

/// A name safe to show and to save under on the phone.
pub(super) fn clean_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base.chars().filter(|c| !c.is_control()).take(120).collect();
    let cleaned = cleaned.trim().trim_start_matches('.');
    if cleaned.is_empty() {
        "file".to_owned()
    } else {
        cleaned.to_owned()
    }
}

/// The MIME type for a file with no usable one, from its extension or PDF magic bytes.
fn mime_for(name: &str, data: &[u8]) -> &'static str {
    if data.starts_with(b"%PDF") {
        return "application/pdf";
    }
    let extension = name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "txt" | "md" => "text/plain",
        "csv" => "text/csv",
        "json" => "application/json",
        "ics" => "text/calendar",
        "zip" => "application/zip",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
#[path = "../tests/tools_file.rs"]
mod tests;
