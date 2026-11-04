//! Sends the user a file in the chat, or copies a file between the user's machines.

use super::{
    args::text,
    machines::{Machines, Need, copy, transfer},
};
use crate::{fetch, projects::model::MachinePath, user::User, workspace::Workspace};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use flashmind::{
    tools::mcp::{McpContent, McpToolCallResult},
    types::tool::{Tool, ToolContext, ToolResult},
};
use iroh::EndpointId;
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
    /// In a project chat, the checkouts by machine that a copy must land inside.
    pub(crate) copy_into: Option<Vec<(EndpointId, MachinePath)>>,
}

/// Refuses a copy into `folder` on `target` outside the checkouts `copy_into` lists.
fn copy_reaches(
    copy_into: Option<&[(EndpointId, MachinePath)]>,
    target: &EndpointId,
    folder: &str,
) -> Result<()> {
    let Some(checkouts) = copy_into else {
        return Ok(());
    };
    let inside = MachinePath::parse(folder).is_ok_and(|folder| {
        checkouts
            .iter()
            .any(|(machine, checkout)| machine == target && checkout.contains(&folder))
    });
    if !inside {
        bail!(
            "in a project chat a file is copied only into one of the project's checkouts, \
             such as a worker's worktree; {folder} is outside them"
        );
    }
    Ok(())
}

/// Where the file comes from, by which arguments were given.
enum Source<'a> {
    /// The research's paper, typeset.
    Paper,
    Url(&'a str),
    /// A file in the user's home folder, or on the machine `from` names.
    User {
        from: Option<&'a str>,
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
            return "Send the user a file, such as a PDF, in this chat; it shows as a file they can open or share. For an image to look at, use send_image instead. Use it whenever the user asks you to send, give or share a file with them, rather than emailing it. Give exactly one source: `paper: true` for this research's paper as a typeset PDF, with its sources; `url` for a file at an https URL; `path` for a file in the user's home folder, where exec runs (relative to it, or absolute), or on the machine `from` names; or `email_id` with `blob_id` for a Fastmail attachment. Files up to 20 MiB. To send a file or a whole folder from one of the user's machines to another instead, such as a render to the machine that needs it, give `path`, `from` (omit for the user's own environment) and `to`, the machine it goes to; it lands in `dst_folder` there (an existing folder, default the home folder), sent machine to machine. Nothing is replaced: a taken name gets a number, as `report (2).pdf`, and the result says where it landed. Both paths must be in folders the machines share.";
        }
        "Send the user a file, such as a PDF, in this chat; it shows as a file they can open or share. For an image to look at, use send_image instead. Use it whenever the user asks you to send, give or share a file with them, rather than emailing it. Give exactly one source: `url` for a file at an https URL, such as the s3url GMAIL_GET_ATTACHMENT or OUTLOOK_DOWNLOAD_OUTLOOK_ATTACHMENT returns; `path` for a file in the user's home folder, where exec runs (relative to it, or absolute), or on the machine `from` names; or `email_id` with `blob_id` for a Fastmail attachment, as FASTMAIL_READ_EMAIL lists it. Files up to 20 MiB. To send a file or a whole folder from one of the user's machines to another instead, such as a render to the machine that needs it, give `path`, `from` (omit for the user's own environment) and `to`, the machine it goes to; it lands in `dst_folder` there (an existing folder, default the home folder), sent machine to machine. Nothing is replaced: a taken name gets a number, as `report (2).pdf`, and the result says where it landed. Both paths must be in folders the machines share."
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
        // A folder may be many gigabytes, and some machines are capped at 1 MiB/s.
        Some(6 * 60 * 60)
    }
    fn humanize(&self, args: &Value) -> String {
        if args.get("paper").and_then(Value::as_bool) == Some(true) {
            return "Typesetting the paper…".into();
        }
        if let Some(to) = to_machine(args) {
            return format!("Sending to {to}");
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
                "path": { "type": "string", "description": "The file on the `from` machine, such as report.pdf in the user's home folder, or a folder when sending to a machine; `~` is the home folder there" },
                "from": { "type": "string", "description": "The machine `path` is on, by its name in the machines list; omit for the user's own environment" },
                "to": { "type": "string", "description": "\"user\" (the default) to send the file in this chat, or a machine's name to copy it there" },
                "dst_folder": { "type": "string", "description": "The folder on `to` it lands in, such as ~/Downloads; it must exist. Default the home folder" },
                "email_id": { "type": "string", "description": "Fastmail email id the attachment belongs to" },
                "blob_id": { "type": "string", "description": "The Fastmail attachment's blobId" },
                "account": { "type": "string", "description": "The Fastmail account the email is in, when the FASTMAIL tools take an `account`" },
                "filename": { "type": "string", "description": "Name to show, such as 'BoardingPass.pdf'; taken from the source when left out" }
            },
            "additionalProperties": false
        })
    }

    async fn send(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let arg = |key| text(&ctx.args, key);
        let paper = self.paper && ctx.args.get("paper").and_then(Value::as_bool) == Some(true);
        if let Err(e) = no_old_args(&ctx.args) {
            return Ok(ToolResult::failure(ctx.tool_call_id, format!("{e:#}")));
        }
        if let Some(to) = to_machine(&ctx.args) {
            let result = self.copy(&ctx, to).await;
            return Ok(match result {
                Ok(text) => ToolResult::success(ctx.tool_call_id, text),
                Err(e) => ToolResult::failure(ctx.tool_call_id, format!("{e:#}")),
            });
        }
        if arg("dst_folder").is_some() {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                "`dst_folder` is for sending to a machine with `to`.",
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
            (None, Some(path), None, None) => Source::User {
                from: arg("from"),
                path,
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
            Source::User { from, path } => {
                let file = read_user_file(
                    UserFiles {
                        workspace: self.workspace.as_deref(),
                        machines: self.machines.as_ref(),
                    },
                    from,
                    path,
                )
                .await?;
                Ok(Loaded {
                    name: file.name,
                    mime: None,
                    data: file.data,
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

    /// Sends `path` on `from` to the machine `to`; the text is the result.
    async fn copy(&self, ctx: &ToolContext<'_>, to: &str) -> Result<String> {
        let args = &ctx.args;
        let arg = |key| text(args, key);
        let path = arg("path").context("sending to a machine takes `path`, what to send")?;
        if arg("url").is_some() || arg("email_id").is_some() {
            bail!("sending to a machine takes only `path` and `from` as the source");
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
        let folder = transfer::folder_on_machine(
            arg("dst_folder").unwrap_or("~"),
            target.cwd(),
            target.home.as_deref(),
        )?;
        let target_id: EndpointId = target
            .endpoint
            .parse()
            .context("the machine's stored key does not parse")?;
        copy_reaches(self.copy_into.as_deref(), &target_id, &folder)?;
        let mut progress = |line: String| ctx.progress(&line);
        let sent = transfer::send(
            &source,
            &src,
            &target,
            &folder,
            &mut progress,
            ctx.cancel_token(),
        )
        .await?;
        let what = match sent.files {
            1 => transfer::size(sent.size),
            files => format!("{files} files, {}", transfer::size(sent.size)),
        };
        let mut text = format!(
            "Sent {what} from {}:{src} to {}:{}.",
            source.shown(),
            target.shown(),
            sent.path
        );
        if sent.skipped > 0 {
            text.push_str(&format!(
                " Left out {} symlinks or special files.",
                sent.skipped
            ));
        }
        if !sent.direct {
            text.push_str(
                " It went through the server: a machine needs a newer beep-node to send directly.",
            );
        }
        Ok(text)
    }
}

/// Where a tool reads the user's files from.
#[derive(Clone, Copy)]
pub(super) struct UserFiles<'a> {
    /// The user's home folder, their own environment.
    pub(super) workspace: Option<&'a Workspace>,
    /// The user's machines, when this turn may use them.
    pub(super) machines: Option<&'a Machines>,
}

/// A file read from the user's home folder or one of their machines.
pub(super) struct UserFile {
    pub(super) name: String,
    pub(super) data: Vec<u8>,
}

/// Reads `path` from machine `from`, or the user's home folder; refuses files over `MAX_DOWNLOAD`.
pub(super) async fn read_user_file(
    files: UserFiles<'_>,
    from: Option<&str>,
    path: &str,
) -> Result<UserFile> {
    let Some(from) = from else {
        let workspace = files
            .workspace
            .context("the user has no environment online to keep files in")?;
        let file = workspace.read(path).await?;
        return Ok(UserFile {
            name: file.name,
            data: file.data,
        });
    };
    let machines = files
        .machines
        .context("this turn cannot use the user's machines")?;
    let picked = machines
        .pick(Some(from), Need::Files)
        .await
        .map_err(anyhow::Error::msg)?;
    let path = copy::picked_path(&picked, path)?;
    let data = copy::read_all(&picked.online, &path, fetch::MAX_DOWNLOAD).await?;
    Ok(UserFile {
        name: copy::file_name(&path).unwrap_or("file").to_owned(),
        data,
    })
}

/// Refuses the old send-to-machine arguments, so a call using them does not do something else.
fn no_old_args(args: &Value) -> Result<()> {
    for old in ["dst_path", "overwrite"] {
        if args.get(old).is_some_and(|value| !value.is_null()) {
            bail!(
                "`{old}` is no longer taken: give `dst_folder`, the existing folder on `to` it lands in. \
                 Nothing is replaced; a taken name gets a number, and the result says where it landed."
            );
        }
    }
    Ok(())
}

/// The machine a call copies to, when `to` names one rather than the user.
fn to_machine(args: &Value) -> Option<&str> {
    args.get("to")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|to| !to.is_empty() && !to.eq_ignore_ascii_case("user"))
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
mod tests {
    use super::*;
    use flashmind::tools::mcp::McpBlob;
    use iroh::SecretKey;

    #[test]
    fn a_project_copy_lands_only_in_a_checkout() {
        let (build, other) = (
            SecretKey::from_bytes(&[1; 32]).public(),
            SecretKey::from_bytes(&[2; 32]).public(),
        );
        let checkouts = vec![(
            build,
            MachinePath::parse("/srv/project/widget").expect("a path"),
        )];
        let into = Some(checkouts.as_slice());
        assert!(copy_reaches(into, &build, "/srv/project/widget/.worktrees/a/.beep").is_ok());
        assert!(copy_reaches(into, &build, "/srv/project/widget").is_ok());
        assert!(copy_reaches(into, &build, "/srv/project/widget-old").is_err());
        assert!(copy_reaches(into, &build, "/srv/project").is_err());
        assert!(copy_reaches(into, &build, "/srv/project/widget/../x").is_err());
        assert!(copy_reaches(into, &other, "/srv/project/widget").is_err());
        // Outside a project chat it goes anywhere.
        assert!(copy_reaches(None, &other, "/tmp").is_ok());
    }

    #[test]
    fn a_pdf_resource_is_read_from_its_file_block() {
        let pdf = b"%PDF-1.7 pass";
        let result = McpToolCallResult {
            content: vec![McpContent::Blob(McpBlob {
                uri: Some("fastmail:blob".into()),
                mime_type: Some("application/pdf".into()),
                data: STANDARD.encode(pdf),
            })],
            is_error: false,
        };
        let found = attachment_in(result).expect("a file");
        assert_eq!(found.data, pdf);
        assert_eq!(found.mime.as_deref(), Some("application/pdf"));
    }

    #[test]
    fn names_lose_folders_and_leading_dots() {
        assert_eq!(clean_name("../../etc/passwd"), "passwd");
        assert_eq!(clean_name(".hidden"), "hidden");
        assert_eq!(clean_name("  "), "file");
        assert_eq!(clean_name("Boarding Pass.pdf"), "Boarding Pass.pdf");
    }
}
