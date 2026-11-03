//! OpenAI image generation, without the `response_format` GPT image models refuse.

use super::{call::Call, job::Produced};
use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use flashmind::types::{CompletionStream, StreamEvent};
use futures::stream;
use reqwest::{Client, Url};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// How long one generation may take; high quality runs past a minute.
const REQUEST_TIME: Duration = Duration::from_secs(170);

/// What OpenAI is sent.
#[derive(Debug, Serialize)]
pub(super) struct ImageBody {
    model: String,
    prompt: String,
    n: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<String>,
    output_format: &'static str,
}

impl ImageBody {
    pub(super) fn new(call: &Call) -> Self {
        Self {
            model: call.model.clone(),
            prompt: call.prompt.clone(),
            n: 1,
            size: call
                .size
                .clone()
                .or_else(|| call.aspect_ratio.as_deref().and_then(size_for)),
            output_format: "png",
        }
    }
}

/// The size GPT image models make closest to `ratio`, such as `16:9`.
fn size_for(ratio: &str) -> Option<String> {
    let (w, h) = ratio.split_once(':')?;
    let (w, h): (f64, f64) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let size = match w / h {
        r if r > 1.2 => "1536x1024",
        r if r < 1.0 / 1.2 => "1024x1536",
        _ => "1024x1024",
    };
    Some(size.to_owned())
}

#[derive(Deserialize)]
struct ImagesResponse {
    data: Vec<ImageData>,
    #[serde(default)]
    output_format: Option<String>,
}

#[derive(Deserialize)]
struct ImageData {
    b64_json: Option<String>,
}

/// The image in an `ImagesResponse` body, with its media type.
pub(super) fn parse(body: &[u8]) -> Result<Produced> {
    let response: ImagesResponse =
        serde_json::from_slice(body).context("OpenAI sent an answer that is not an image list")?;
    let encoded = response
        .data
        .into_iter()
        .find_map(|image| image.b64_json)
        .context("OpenAI sent no image")?;
    let data = STANDARD
        .decode(encoded)
        .context("OpenAI's image is not base64")?;
    let media_type = match response.output_format.as_deref() {
        Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => "image/png",
    };
    Ok(Produced {
        media_type: media_type.to_owned(),
        data,
    })
}

/// Asks OpenAI at `base` (its `/v1` root) for the image in `body`.
pub(super) fn generate(base: &Url, key: SecretString, body: ImageBody) -> CompletionStream {
    let url = format!("{}/images/generations", base.as_str().trim_end_matches('/'));
    Box::pin(stream::once(async move {
        let Produced { media_type, data } = request(&url, &key, &body).await?;
        Ok(StreamEvent::FileAttachment {
            filename: String::new(),
            media_type,
            data,
        })
    }))
}

async fn request(url: &str, key: &SecretString, body: &ImageBody) -> Result<Produced> {
    let client = Client::builder().timeout(REQUEST_TIME).build()?;
    let response = client
        .post(url)
        .bearer_auth(key.expose_secret())
        .json(body)
        .send()
        .await
        .map_err(reqwest::Error::without_url)
        .context("could not reach OpenAI")?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .context("could not read OpenAI's answer")?;
    if !status.is_success() {
        return Err(refused(status.as_u16(), &bytes));
    }
    parse(&bytes)
}

#[derive(Deserialize)]
struct ErrorBody {
    error: ErrorMessage,
}

#[derive(Deserialize)]
struct ErrorMessage {
    message: String,
}

/// OpenAI's refusal as one line: its own message when it gave one.
fn refused(status: u16, body: &[u8]) -> anyhow::Error {
    match serde_json::from_slice::<ErrorBody>(body) {
        Ok(error) => anyhow::anyhow!("OpenAI refused it ({status}): {}", error.error.message),
        Err(_) => anyhow::anyhow!("OpenAI refused it ({status})"),
    }
}
