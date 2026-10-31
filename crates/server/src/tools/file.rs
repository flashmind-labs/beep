//! Sends the user a file in the chat, such as a mail attachment or a file
//! a command made, instead of mailing it to them; or copies a file from one
//! of the user's machines to another.

use super::machines::{Machines, Need, copy};
use crate::{fetch, user::User, workspace::Workspace};
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
    pub(crate) workspace: Option<Arc<Workspace>>,
    /// The user's machines, when this turn may use them.
    pub(crate) machines: Option<Machines>,
    /// The chat is a research, so its paper can be sent as a PDF.
    pub(crate) paper: bool,
}

/// Where the file comes from, by which arguments were given.
enum Source<'a> {
    /// The research's paper, typeset.
    Paper,
    Url(&'a str),
    Folder(&'a str),
    /// A file on the machine `from` names.
    Machine {
        from: &'a str,
        path: &'a str,
    },
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
            return "Send the user a file, such as a PDF, in this chat; it shows as a file they can open or share. For an image to look at, use send_image instead. Use it whenever the user asks you to send, give or share a file with them, rather than emailing it. Give exactly one source: `paper: true` for this research's paper as a typeset PDF, with its sources; `url` for a file at an https URL; `path` for a file in the user's home folder, where exec runs (relative to it, or absolute), or on the machine `from` names; or `email_id` with `blob_id` for a Fastmail attachment. Files up to 20 MiB. To copy a file between the user's machines instead, give `path`, `from` (omit for the user's own environment) and `to`, the machine it goes to; it lands at `~/<file name>` there unless `dst_path` says where, folders are made as needed, an existing file is replaced only with `overwrite: true`, and files up to 2 GiB copy. Folders do not copy: tar one with exec first.";
        }
        "Send the user a file, such as a PDF, in this chat; it shows as a file they can open or share. For an image to look at, use send_image instead. Use it whenever the user asks you to send, give or share a file with them, rather than emailing it. Give exactly one source: `url` for a file at an https URL, such as the s3url GMAIL_GET_ATTACHMENT or OUTLOOK_DOWNLOAD_OUTLOOK_ATTACHMENT returns; `path` for a file in the user's home folder, where exec runs (relative to it, or absolute), or on the machine `from` names; or `email_id` with `blob_id` for a Fastmail attachment, as FASTMAIL_READ_EMAIL lists it. Files up to 20 MiB. To copy a file between the user's machines instead, give `path`, `from` (omit for the user's own environment) and `to`, the machine it goes to; it lands at `~/<file name>` there unless `dst_path` says where, folders are made as needed, an existing file is replaced only with `overwrite: true`, and files up to 2 GiB copy. Folders do not copy: tar one with exec first."
    }
    fn parameters(&self) -> Value {
        let mut schema = self.schema();
        if self.paper {
            schema["properties"]["paper"] = json!({
                "type": "boolean",
                "description": "true to send this research's paper as a PDF"
            });
        }
        schema
    }
    fn timeout_secs(&self) -> Option<u64> {
        // A copy between machines may move up to 2 GiB.
        Some(30 * 60)
    }
    fn humanize(&self, args: &Value) -> String {
        if args.get("paper").and_then(Value::as_bool) == Some(true) {
            return "Typesetting the paper…".into();
        }
        if let Some(to) = to_machine(args) {
            return format!("Copying a file to {to}");
        }
        "Sending file…".into()
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        self.send(ctx).await
    }
}

impl SendFileTool {
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "https URL of the file" },
                "path": { "type": "string", "description": "The file on the `from` machine, such as report.pdf in the user's home folder; `~` is the home folder there" },
                "from": { "type": "string", "description": "The machine `path` is on, by its name in the machines list; omit for the user's own environment" },
                "to": { "type": "string", "description": "\"user\" (the default) to send the file in this chat, or a machine's name to copy it there" },
                "dst_path": { "type": "string", "description": "Where the copy lands on `to`, such as ~/Downloads/report.pdf; default ~/<file name>" },
                "overwrite": { "type": "boolean", "description": "Replace an existing file at the destination" },
                "email_id": { "type": "string", "description": "Fastmail email id the attachment belongs to" },
                "blob_id": { "type": "string", "description": "The Fastmail attachment's blobId" },
                "account": { "type": "string", "description": "The Fastmail account the email is in, when the FASTMAIL tools take an `account`" },
                "filename": { "type": "string", "description": "Name to show, such as 'BoardingPass.pdf'; taken from the source when left out" }
            },
            "additionalProperties": false
        })
    }

    async fn send(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let arg = |key| {
            ctx.args
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
        };
        let paper = self.paper && ctx.args.get("paper").and_then(Value::as_bool) == Some(true);
        if let Some(to) = to_machine(&ctx.args) {
            let result = self.copy(&ctx.args, to).await;
            return Ok(match result {
                Ok(text) => ToolResult::success(ctx.tool_call_id, text),
                Err(e) => ToolResult::failure(ctx.tool_call_id, format!("{e:#}")),
            });
        }
        if arg("dst_path").is_some() {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                "`dst_path` is for copying to a machine with `to`.",
            ));
        }
        let source = match (arg("url"), arg("path"), arg("email_id"), arg("blob_id")) {
            (None, None, None, None) if paper => Source::Paper,
            _ if paper => {
                return Ok(ToolResult::failure(
                    ctx.tool_call_id,
                    "Give `paper` alone, or another source without it.",
                ));
            }
            (Some(url), None, None, None) => Source::Url(url),
            (None, Some(path), None, None) => match arg("from") {
                Some(from) => Source::Machine { from, path },
                None => Source::Folder(path),
            },
            (None, None, Some(email_id), Some(blob_id)) => Source::Fastmail {
                account: arg("account"),
                email_id,
                blob_id,
            },
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
            Source::Folder(path) => {
                let workspace = self
                    .workspace
                    .as_ref()
                    .context("the user has no environment online to keep files in")?;
                let file = workspace.read(path).await?;
                Ok(Loaded {
                    name: file.name,
                    mime: None,
                    data: file.data,
                })
            }
            Source::Machine { from, path } => {
                let machines = self.machines()?;
                let picked = machines
                    .pick(Some(from), Need::Files)
                    .await
                    .map_err(anyhow::Error::msg)?;
                let path = copy::picked_path(&picked, path)?;
                let data = copy::read_all(&picked.online, &path, fetch::MAX_DOWNLOAD).await?;
                Ok(Loaded {
                    name: copy::file_name(&path).unwrap_or("file").to_owned(),
                    mime: None,
                    data,
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

impl SendFileTool {
    fn machines(&self) -> Result<&Machines> {
        self.machines
            .as_ref()
            .context("this turn cannot use the user's machines")
    }

    /// Copies `path` on `from` to the machine `to`; the text is the result.
    async fn copy(&self, args: &Value, to: &str) -> Result<String> {
        let arg = |key| {
            args.get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
        };
        let path = arg("path").context("copying to a machine takes `path`, the file to copy")?;
        if arg("url").is_some() || arg("email_id").is_some() {
            bail!("copying to a machine takes only `path` and `from` as the source");
        }
        let machines = self.machines()?;
        let source = machines
            .pick(arg("from"), Need::Files)
            .await
            .map_err(anyhow::Error::msg)?;
        let target = machines
            .pick(Some(to), Need::Files)
            .await
            .map_err(anyhow::Error::msg)?;
        let src = copy::picked_path(&source, path)?;
        let dst = match arg("dst_path") {
            Some(dst) => copy::picked_path(&target, dst)?,
            None => {
                let name = copy::file_name(&src).context("the path names no file")?;
                copy::picked_path(&target, &format!("~/{name}"))?
            }
        };
        let overwrite = args.get("overwrite").and_then(Value::as_bool) == Some(true);
        let copied = copy::copy(&source.online, &src, &target.online, &dst, overwrite).await?;
        Ok(format!(
            "Copied {} bytes from {}:{src} to {}:{}.",
            copied.bytes,
            source.shown(),
            target.shown(),
            copied.path
        ))
    }
}

/// The machine a call copies to, when `to` names one rather than the user.
fn to_machine(args: &Value) -> Option<&str> {
    args.get("to")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|to| !to.is_empty() && !to.eq_ignore_ascii_case("user"))
}

/// The Fastmail attachment `blob_id` of `email_id`, in `account` or the
/// first account, as the file itself rather than the text `read_attachment`
/// gives by default.
pub(crate) async fn fastmail_attachment(
    user: &User,
    account: Option<&str>,
    email_id: &str,
    blob_id: &str,
) -> Result<Attachment> {
    let args = json!({ "emailId": email_id, "blobId": blob_id, "format": "original" });
    let result = user.call_fastmail(account, "read_attachment", args).await?;
    let attachment = attachment_in(&result)
        .context("Fastmail only gives the file itself for PDFs and images")?;
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
    fn to_names_a_machine_or_the_user() {
        assert_eq!(to_machine(&json!({ "to": "mac" })), Some("mac"));
        assert_eq!(to_machine(&json!({ "to": "User" })), None);
        assert_eq!(to_machine(&json!({ "to": " " })), None);
        assert_eq!(to_machine(&json!({ "path": "a.pdf" })), None);
    }

    #[test]
    fn a_pdf_is_known_by_its_bytes() {
        assert_eq!(mime_for("download", b"%PDF-1.4"), "application/pdf");
        assert_eq!(mime_for("photo.JPG", b""), "image/jpeg");
        assert_eq!(mime_for("x", b""), "application/octet-stream");
    }
}
