use crate::sandbox::{content_type, follow_public};
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use reqwest::{StatusCode, Url};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::timeout;

const MAX_IMAGES: usize = 6;
/// How long checking one image may take, redirects included.
const CHECK_TIME: Duration = Duration::from_secs(10);

/// Shows images in the chat. Each URL is fetched first, so a made-up link
/// fails here instead of showing as a broken image on the phone.
pub(crate) struct SendImageTool;

#[async_trait]
impl Tool for SendImageTool {
    fn name(&self) -> &str {
        "send_image"
    }
    fn description(&self) -> &str {
        "Send images to the user; they show in the chat as pictures. Only use https image URLs you got from a tool, a page or the user, never one you made up. Each URL is checked, and ones that are not images are reported back."
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
                            "caption": { "type": "string", "maxLength": 200 }
                        },
                        "required": ["url"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["images"],
            "additionalProperties": false
        })
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
        let checks = requested.iter().map(|image| async move {
            let url = image
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            let caption = image
                .get("caption")
                .and_then(Value::as_str)
                .unwrap_or("")
                .chars()
                .take(200)
                .collect::<String>();
            match check(url).await {
                Ok(()) => Ok(json!({ "url": url, "caption": caption })),
                Err(reason) => Err(format!("{url}: {reason}")),
            }
        });
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
}
