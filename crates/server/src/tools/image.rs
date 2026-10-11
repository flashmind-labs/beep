use super::file::clean_name;
use crate::{
    sandbox::{Sandbox, content_type, follow_public},
    user::User,
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

/// Shows images in the chat, from the web or from the user's file folder.
/// Each URL is fetched first, so a made-up link fails here instead of
/// showing as a broken image on the phone. A folder image is kept with the
/// chat, as a sent file is, and the phone shows it as a picture.
pub(crate) struct SendImageTool {
    pub(crate) user: Arc<User>,
    pub(crate) session_id: Uuid,
    pub(crate) sandbox: Option<Arc<Sandbox>>,
}

#[async_trait]
impl Tool for SendImageTool {
    fn name(&self) -> &str {
        "send_image"
    }
    fn description(&self) -> &str {
        "Send images to the user; they show in the chat as pictures. Give each one either `url`, an https image URL you got from a tool, a page or the user and never one you made up, or `path`, a PNG, JPEG, GIF or WebP in the user's file folder, such as a chart execute_code drew. Use this rather than send_file for images. Each one is checked, and ones that are not images are reported back."
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
                            "path": { "type": "string", "description": "Image in the user's file folder, as execute_code or download_file named it" },
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
        let sandbox = self
            .sandbox
            .as_ref()
            .ok_or("this server has no file folder")?;
        let file = sandbox
            .read(self.user.external_id(), path)
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
fn image_mime(data: &[u8]) -> Option<&'static str> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
