use super::file::clean_name;
use crate::{
    fetch::{content_type, follow_public},
    user::User,
    workspace::Workspace,
};
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use imagesize::ImageType;
use reqwest::{StatusCode, Url};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::time::timeout;
use uuid::Uuid;

const MAX_IMAGES: usize = 6;
/// How long checking one image may take, redirects included.
const CHECK_TIME: Duration = Duration::from_secs(10);

/// The tool's name, which the apps and the routine notice look for.
pub(crate) const NAME: &str = "send_image";

/// Shows images in the chat, from the web or from the user's home folder.
/// Each URL is fetched first, so a made-up link fails here instead of
/// showing as a broken image on the phone. A folder image is kept with the
/// chat, as a sent file is, and the phone shows it as a picture.
pub(crate) struct SendImageTool {
    pub(crate) user: Arc<User>,
    pub(crate) session_id: Uuid,
    pub(crate) workspace: Option<Arc<Workspace>>,
}

#[async_trait]
impl Tool for SendImageTool {
    fn name(&self) -> &str {
        NAME
    }
    fn description(&self) -> &str {
        "Send images to the user; they show in the chat as pictures. Give each one either `url`, an https image URL you got from a tool, a page or the user and never one you made up, or `path`, a PNG, JPEG, GIF or WebP in the user's home folder (relative to it, or absolute), such as a chart a command drew with exec. Use this rather than send_file for images. Each one is checked, and ones that are not images are reported back."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "images": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": MAX_IMAGES,
                    "items": {
                        "type": "object",
                        "properties": {
                            "url": { "type": "string", "description": "https URL of the image file" },
                            "path": { "type": "string", "description": "Image in the user's home folder, such as chart.png" },
                            "caption": { "type": "string", "maxLength": 200 }
                        },
                        "additionalProperties": false
                    }
                }
            },
            "required": ["images"],
            "additionalProperties": false
        })
    }
    fn timeout_secs(&self) -> Option<u64> {
        Some(60)
    }
    fn humanize(&self, _args: &Value) -> String {
        "Sending images…".into()
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let requested = ctx
            .args
            .get("images")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if requested.is_empty() || requested.len() > MAX_IMAGES {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                format!("Send between 1 and {MAX_IMAGES} images."),
            ));
        }
        let checks = requested.iter().map(|image| self.send_one(image));
        let mut images = Vec::new();
        let mut failed = Vec::new();
        for result in futures::future::join_all(checks).await {
            match result {
                Ok(image) => images.push(image),
                Err(reason) => failed.push(reason),
            }
        }
        if images.is_empty() {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                format!("No image could be sent. {}", failed.join("; ")),
            ));
        }
        let output = json!({ "action": "send_image", "images": images, "failed": failed });
        Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
    }
}

impl SendImageTool {
    /// Checks or saves one requested image, and returns it as the phone reads
    /// it, or why it cannot be sent.
    async fn send_one(&self, image: &Value) -> Result<Value, String> {
        let field = |key| {
            image
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
        };
        let caption: String = field("caption").unwrap_or("").chars().take(200).collect();
        match (field("url"), field("path")) {
            (Some(url), None) => match check(url).await {
                Ok(()) => Ok(json!({ "url": url, "caption": caption })),
                Err(reason) => Err(format!("{url}: {reason}")),
            },
            (None, Some(path)) => match self.save(path).await {
                Ok(file) => Ok(json!({ "file": file, "caption": caption })),
                Err(reason) => Err(format!("{path}: {reason}")),
            },
            _ => Err("give each image either url or path".to_owned()),
        }
    }

    /// Keeps the folder image at `path` with the chat, and returns it as a
    /// sent file.
    async fn save(&self, path: &str) -> Result<Value, String> {
        let workspace = self
            .workspace
            .as_ref()
            .ok_or("the user has no environment online to keep files in")?;
        let file = workspace
            .read(path)
            .await
            .map_err(|error| format!("{error:#}"))?;
        let mime = image_mime(&file.data).ok_or("it is not a PNG, JPEG, GIF or WebP image")?;
        let name = clean_name(&file.name);
        let id = self
            .user
            .save_file(self.session_id, &name, mime, &file.data)
            .await
            .map_err(|error| format!("could not keep it: {error:#}"))?;
        Ok(json!({ "id": id, "name": name, "mime": mime, "size": file.data.len() }))
    }
}

/// The MIME type of an image the phone can show, by its first bytes.
pub(super) fn image_mime(data: &[u8]) -> Option<&'static str> {
    match imagesize::image_type(data).ok()? {
        ImageType::Png => Some("image/png"),
        ImageType::Jpeg => Some("image/jpeg"),
        ImageType::Gif => Some("image/gif"),
        ImageType::Webp => Some("image/webp"),
        _ => None,
    }
}

/// Checks that `url` is a public https address serving an image.
pub(super) async fn check(url: &str) -> Result<(), &'static str> {
    let parsed = Url::parse(url).map_err(|_| "not a valid URL")?;
    if parsed.scheme() != "https" || url.len() > 2048 {
        return Err("only https URLs up to 2048 bytes are allowed");
    }
    // Model input can be steered by pages it read, so keep the server off
    // addresses that only make sense on its own network, through every
    // redirect. Only the headers are read.
    let response = timeout(CHECK_TIME, follow_public(url))
        .await
        .map_err(|_| "could not be fetched in time")?
        .map_err(|_| "could not be fetched from a public address")?
        .response;
    // Shop CDNs such as Chanel's turn away datacenter addresses whatever the
    // request looks like, while the phone gets the same image fine. A refusal
    // says nothing about the image, so leave it to the phone.
    if matches!(
        response.status(),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS
    ) {
        return Ok(());
    }
    if !response.status().is_success() {
        return Err("the server did not return it");
    }
    let is_image = content_type(&response).is_some_and(|v| v.trim_start().starts_with("image/"));
    if !is_image {
        return Err("it is not an image");
    }
    Ok(())
}

/// How much of a page is read looking for its preview image, which sits in
/// the head.
const MAX_HEAD: usize = 512 * 1024;

/// The preview image a page names for link previews (`og:image`, else
/// `twitter:image`), checked to load. For a card the model gave no picture.
pub(super) async fn page_image(page: &str) -> Option<String> {
    let head = timeout(CHECK_TIME, read_head(page)).await.ok()??;
    let base = Url::parse(page).ok()?;
    let url = base.join(&preview_image(&head)?).ok()?;
    check(url.as_str()).await.ok()?;
    Some(url.into())
}

/// The start of `page`'s HTML, up to its `</head>` or `MAX_HEAD` bytes.
async fn read_head(page: &str) -> Option<String> {
    let mut response = follow_public(page).await.ok()?.response;
    let is_html = content_type(&response).is_some_and(|v| v.contains("html"));
    if !response.status().is_success() || !is_html {
        return None;
    }
    let mut head = Vec::new();
    while head.len() < MAX_HEAD {
        let Ok(Some(chunk)) = response.chunk().await else {
            break;
        };
        head.extend_from_slice(&chunk);
        if head.windows(7).any(|w| w.eq_ignore_ascii_case(b"</head>")) {
            break;
        }
    }
    Some(String::from_utf8_lossy(&head).into_owned())
}

/// The `content` of the first `og:image` meta tag in `html`, else of the
/// first `twitter:image`.
fn preview_image(html: &str) -> Option<String> {
    let tags: Vec<&str> = html
        .match_indices("<meta")
        .filter_map(|(at, _)| html[at..].find('>').map(|end| &html[at..at + end]))
        .collect();
    ["og:image", "twitter:image"].iter().find_map(|name| {
        tags.iter().find_map(|tag| {
            let named = ["property", "name"]
                .iter()
                .any(|key| attribute(tag, key).is_some_and(|v| v.eq_ignore_ascii_case(name)));
            named
                .then(|| attribute(tag, "content"))
                .flatten()
                .filter(|v| !v.is_empty())
                .map(|v| v.replace("&amp;", "&"))
        })
    })
}

/// The value of attribute `key` in one tag, quoted with `"` or `'`.
fn attribute<'a>(tag: &'a str, key: &str) -> Option<&'a str> {
    let mut rest = tag;
    while let Some(at) = rest.find(key) {
        let before = rest[..at].chars().next_back();
        let after = rest[at + key.len()..].trim_start();
        rest = &rest[at + key.len()..];
        if !before.is_some_and(char::is_whitespace) {
            continue;
        }
        let Some(value) = after.strip_prefix('=') else {
            continue;
        };
        let value = value.trim_start();
        let quote = value.chars().next()?;
        if quote != '"' && quote != '\'' {
            continue;
        }
        let value = &value[1..];
        return value.find(quote).map(|end| value[..end].trim());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_pages_preview_image() {
        let html = r#"<head><meta name="twitter:image" content="https://cdn.example/t.jpg">
            <meta content='/img/a.jpg?w=1&amp;h=2' property='og:image' />
            <meta property="og:image:width" content="1200"></head>"#;
        assert_eq!(preview_image(html).as_deref(), Some("/img/a.jpg?w=1&h=2"));
        let html = r#"<meta name="twitter:image" content="https://cdn.example/t.jpg">"#;
        assert_eq!(
            preview_image(html).as_deref(),
            Some("https://cdn.example/t.jpg")
        );
        assert_eq!(
            preview_image(r#"<meta property="og:title" content="A">"#),
            None
        );
    }

    #[tokio::test]
    async fn rejects_unsafe_urls_without_fetching() {
        for url in [
            "http://example.com/a.png",
            "https://127.0.0.1/a.png",
            "https://[::1]/a.png",
            "https://localhost/a.png",
            "https://intranet/a.png",
            "https://[fd00::1]/a.png",
            "https://169.254.169.254/latest/meta-data/",
            "file:///etc/passwd",
            "not a url",
        ] {
            assert!(check(url).await.is_err(), "{url}");
        }
    }

    #[test]
    fn folder_images_are_known_by_their_bytes() {
        let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0";
        assert_eq!(image_mime(png), Some("image/png"));
        assert_eq!(image_mime(b"%PDF-1.7"), None);
        assert_eq!(image_mime(b""), None);
    }
}
