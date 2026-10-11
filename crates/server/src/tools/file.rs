//! Sends the user a file in the chat, such as a mail attachment or a file
//! code made, instead of mailing it to them.

use crate::{apps::mcp::FASTMAIL, sandbox, sandbox::Sandbox, user::User};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

pub(crate) const NAME: &str = "send_file";

pub(crate) struct SendFileTool {
    pub(crate) user: Arc<User>,
    pub(crate) session_id: Uuid,
    pub(crate) sandbox: Option<Arc<Sandbox>>,
}

/// Where the file comes from, by which arguments were given.
enum Source<'a> {
    Url(&'a str),
    Folder(&'a str),
    Fastmail { email_id: &'a str, blob_id: &'a str },
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
        "Send the user a file, such as a PDF, in this chat; it shows as a file they can open or share. For an image to look at, use send_image instead. Use it whenever the user asks you to send, give or share a file with them, rather than emailing it. Give exactly one source: `url` for a file at an https URL, such as the s3url GMAIL_GET_ATTACHMENT or OUTLOOK_DOWNLOAD_OUTLOOK_ATTACHMENT returns; `path` for a file in the user's file folder; or `email_id` with `blob_id` for a Fastmail attachment, as FASTMAIL_READ_EMAIL lists it. Files up to 20 MiB."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "https URL of the file" },
                "path": { "type": "string", "description": "File in the user's file folder, as execute_code or download_file named it" },
                "email_id": { "type": "string", "description": "Fastmail email id the attachment belongs to" },
                "blob_id": { "type": "string", "description": "The Fastmail attachment's blobId" },
                "filename": { "type": "string", "description": "Name to show, such as 'BoardingPass.pdf'; taken from the source when left out" }
            },
            "additionalProperties": false
        })
    }
    fn timeout_secs(&self) -> Option<u64> {
        Some(120)
    }
    fn humanize(&self, _args: &Value) -> String {
        "Sending file…".into()
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let arg = |key| {
            ctx.args
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
        };
        let source = match (arg("url"), arg("path"), arg("email_id"), arg("blob_id")) {
            (Some(url), None, None, None) => Source::Url(url),
            (None, Some(path), None, None) => Source::Folder(path),
            (None, None, Some(email_id), Some(blob_id)) => Source::Fastmail { email_id, blob_id },
            _ => {
                return Ok(ToolResult::failure(
                    ctx.tool_call_id,
                    "Give exactly one source: `url`, `path`, or `email_id` with `blob_id`.",
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
}

impl SendFileTool {
    async fn load(&self, source: Source<'_>) -> Result<Loaded> {
        match source {
            Source::Url(url) => {
                if !url.starts_with("https://") {
                    bail!("only https URLs are allowed");
                }
                let fetched = sandbox::fetch_public(url).await?;
                Ok(Loaded {
                    name: fetched.name,
                    mime: fetched
                        .content_type
                        .map(|t| t.split(';').next().unwrap_or("").trim().to_owned()),
                    data: fetched.data,
                })
            }
            Source::Folder(path) => {
                let sandbox = self
                    .sandbox
                    .as_ref()
                    .context("this server has no file folder")?;
                let file = sandbox.read(self.user.external_id(), path).await?;
                Ok(Loaded {
                    name: file.name,
                    mime: None,
                    data: file.data,
                })
            }
            Source::Fastmail { email_id, blob_id } => {
                let args = json!({ "emailId": email_id, "blobId": blob_id, "format": "original" });
                let result = self
                    .user
                    .call_mcp(FASTMAIL, "read_attachment", args)
                    .await?;
                let attachment = attachment_in(&result)
                    .context("Fastmail only gives the file itself for PDFs and images")?;
                if attachment.data.len() as u64 > sandbox::MAX_DOWNLOAD {
                    bail!(
                        "the file is larger than {} MiB",
                        sandbox::MAX_DOWNLOAD >> 20
                    );
                }
                Ok(Loaded {
                    name: attachment.name.unwrap_or_else(|| "attachment".to_owned()),
                    mime: attachment.mime,
                    data: attachment.data,
                })
            }
        }
    }
}

/// A file decoded from an MCP result.
#[derive(Debug, PartialEq)]
struct Attachment {
    name: Option<String>,
    mime: Option<String>,
    data: Vec<u8>,
}

/// The file in Fastmail's `read_attachment` result. The MCP client hands
/// image and resource blocks over only as their Debug text, so the base64
/// is read out of that; JSON with a `data` or `blob` field works as well.
fn attachment_in(result: &Value) -> Option<Attachment> {
    match result {
        Value::String(text) => {
            let encoded =
                quoted_after(text, "blob: \"").or_else(|| quoted_after(text, "data: \""))?;
            let mime = quoted_after(text, "mime_type: Some(\"")
                .or_else(|| quoted_after(text, "mime_type: \""))
                .map(str::to_owned);
            let data = STANDARD.decode(encoded).ok()?;
            Some(Attachment {
                name: None,
                mime,
                data,
            })
        }
        Value::Object(fields) => {
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
        _ => None,
    }
}

/// The text between `prefix` and the next quote. Base64 and MIME types
/// hold no quotes or escapes, so Debug quoting leaves them as they are.
fn quoted_after<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let start = text.find(prefix)? + prefix.len();
    let rest = &text[start..];
    Some(&rest[..rest.find('"')?])
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

/// The MIME type for a file with no usable one, from its extension or, for
/// a PDF, its first bytes.
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
mod tests {
    use super::*;
    use rmcp::model::{ContentBlock, ResourceContents};

    /// What the MCP client gives `call_mcp` for a block that is not text.
    fn as_seen(block: ContentBlock) -> Value {
        Value::String(format!("{block:?}"))
    }

    #[test]
    fn a_pdf_resource_is_read_from_the_debug_text() {
        let pdf = b"%PDF-1.7 pass";
        let block = ContentBlock::resource(ResourceContents::BlobResourceContents {
            uri: "fastmail:blob".into(),
            mime_type: Some("application/pdf".into()),
            blob: STANDARD.encode(pdf),
            meta: None,
        });
        let found = attachment_in(&as_seen(block)).unwrap();
        assert_eq!(found.data, pdf);
        assert_eq!(found.mime.as_deref(), Some("application/pdf"));
    }

    #[test]
    fn an_image_is_read_from_the_debug_text() {
        let block = ContentBlock::image(STANDARD.encode([1, 2, 3]), "image/png");
        let found = attachment_in(&as_seen(block)).unwrap();
        assert_eq!(found.data, [1, 2, 3]);
        assert_eq!(found.mime.as_deref(), Some("image/png"));
    }

    #[test]
    fn json_with_data_is_read_too() {
        let result =
            json!({ "name": "a.pdf", "type": "application/pdf", "data": STANDARD.encode(b"x") });
        let found = attachment_in(&result).unwrap();
        assert_eq!(found.name.as_deref(), Some("a.pdf"));
        assert_eq!(found.data, b"x");
        assert!(attachment_in(&json!({ "text": "just text" })).is_none());
    }

    #[test]
    fn names_lose_folders_and_leading_dots() {
        assert_eq!(clean_name("../../etc/passwd"), "passwd");
        assert_eq!(clean_name(".hidden"), "hidden");
        assert_eq!(clean_name("  "), "file");
        assert_eq!(clean_name("Boarding Pass.pdf"), "Boarding Pass.pdf");
    }

    #[test]
    fn a_pdf_is_known_by_its_bytes() {
        assert_eq!(mime_for("download", b"%PDF-1.4"), "application/pdf");
        assert_eq!(mime_for("photo.JPG", b""), "image/jpeg");
        assert_eq!(mime_for("x", b""), "application/octet-stream");
    }
}
