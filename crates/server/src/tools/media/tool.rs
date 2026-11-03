//! `image_gen`, `video_gen` and `3d_gen`, each offered only with a key for it.

use super::{
    call::{Call, ImageSource, schema},
    job::{JobError, Produced},
    produce::{self, Endpoints, Job},
    provider::{Keys, MediaKind, MediaProvider},
};
use crate::{
    fetch,
    tools::{file::clean_name, image::image_mime},
    user::User,
    workspace::Workspace,
};
use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

/// The most a made file may be: what the chat and the home folder keep.
const MAX_FILE: usize = fetch::MAX_DOWNLOAD as usize;
/// Most words of the prompt a file is named after.
const NAME_WORDS: usize = 6;

/// What every media tool of a turn shares.
pub(crate) struct Media {
    pub(crate) user: Arc<User>,
    pub(crate) chat: Uuid,
    pub(crate) workspace: Option<Arc<Workspace>>,
    pub(crate) keys: Keys,
    pub(crate) endpoints: Endpoints,
}

/// One kind's tool, for the services in `providers`.
pub(crate) struct MediaTool {
    kind: MediaKind,
    providers: Vec<MediaProvider>,
    description: String,
    media: Arc<Media>,
}

impl MediaTool {
    /// The tool for `kind`, or `None` when no key the user has makes it.
    pub(crate) fn new(kind: MediaKind, media: Arc<Media>) -> Option<Self> {
        let providers = media.keys.providers(kind);
        if providers.is_empty() {
            return None;
        }
        Some(Self {
            kind,
            description: description(kind, &providers),
            providers,
            media,
        })
    }
}

fn description(kind: MediaKind, providers: &[MediaProvider]) -> String {
    let what = match kind {
        MediaKind::Image => {
            "Make an image from a prompt, or edit one: give `image` to change it or work from it."
        }
        MediaKind::Video => {
            "Make a short video from a prompt, optionally starting from `image` as its first frame. It takes minutes."
        }
        MediaKind::Model3d => {
            "Make a 3D model (GLB) of an object from a prompt or a picture of it, with Meshy. It takes minutes."
        }
    };
    let services: Vec<String> = providers
        .iter()
        .map(|provider| {
            let model = kind
                .default_model(*provider, false)
                .map(|model| format!(" (default model {model})"))
                .unwrap_or_default();
            format!("{}{model}", provider.slug())
        })
        .collect();
    format!(
        "{what} It runs on the user's own key and costs them money, so make it only when they ask for it. \
         Services with a key: {}. The user sees the result in the chat; it is also saved in their home \
         folder, and the result gives its path for other tools.",
        services.join(", ")
    )
}

#[async_trait]
impl Tool for MediaTool {
    fn name(&self) -> &str {
        self.kind.tool()
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn parameters(&self) -> Value {
        schema(self.kind, &self.providers)
    }
    fn timeout_secs(&self) -> Option<u64> {
        // Past the job's own limit, so its message is the one the model reads.
        Some(self.kind.limit().as_secs() + 120)
    }
    fn humanize(&self, _args: &Value) -> String {
        match self.kind {
            MediaKind::Image => "Making an image…",
            MediaKind::Video => "Making a video…",
            MediaKind::Model3d => "Making a 3D model…",
        }
        .into()
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let call = match Call::parse(self.kind, &self.providers, &ctx.args) {
            Ok(call) => call,
            Err(reason) => return Ok(ToolResult::failure(ctx.tool_call_id, reason)),
        };
        Ok(match self.make(&call, &ctx).await {
            Ok(output) => ToolResult::success(ctx.tool_call_id, output.to_string()),
            Err(error) => ToolResult::failure(
                ctx.tool_call_id,
                format!("{} could not make it: {error:#}", call.provider.name()),
            ),
        })
    }
}

impl MediaTool {
    /// Makes what `call` asks for and delivers it; the value is the result.
    async fn make(&self, call: &Call, ctx: &ToolContext<'_>) -> Result<Value> {
        let key = self
            .media
            .keys
            .get(call.provider)
            .with_context(|| format!("the user has no {} key", call.provider.name()))?;
        let image = match &call.image {
            Some(source) => Some(self.image_input(source).await?),
            None => None,
        };
        let job = Job {
            call,
            image,
            key,
            endpoints: &self.media.endpoints,
        };
        let cancel = ctx.cancel_token();
        let mut progress = |line: &str| ctx.progress(line);
        let produced = match self.kind {
            MediaKind::Model3d => {
                let url = produce::model_3d(job, cancel, &mut progress)
                    .await
                    .map_err(job_error)?;
                ctx.progress("Downloading the model");
                let fetched = fetch::fetch_public(url.as_str())
                    .await
                    .with_context(|| format!("could not download the model from {url}"))?;
                Produced {
                    media_type: GLB.to_owned(),
                    data: fetched.data,
                }
            }
            _ => produce::media(job, cancel, &mut progress)
                .await
                .map_err(job_error)?,
        };
        self.deliver(call, produced).await
    }

    /// The input image as an https URL, or a home folder file as a data URI.
    async fn image_input(&self, source: &ImageSource) -> Result<String> {
        match source {
            ImageSource::Url(url) => Ok(url.clone()),
            ImageSource::Path(path) => {
                let workspace = self
                    .media
                    .workspace
                    .as_ref()
                    .context("the user has no environment online to read the image from")?;
                let file = workspace.read(path).await?;
                let mime = image_mime(&file.data)
                    .with_context(|| format!("{path} is not a PNG, JPEG, GIF or WebP image"))?;
                Ok(format!(
                    "data:{mime};base64,{}",
                    STANDARD.encode(&file.data)
                ))
            }
        }
    }

    /// Keeps the file with the chat and saves it in the home folder.
    async fn deliver(&self, call: &Call, produced: Produced) -> Result<Value> {
        let Produced { media_type, data } = produced;
        ensure!(!data.is_empty(), "the service sent an empty file");
        ensure!(
            data.len() <= MAX_FILE,
            "the file is larger than {} MiB",
            MAX_FILE >> 20
        );
        let mime = match self.kind {
            MediaKind::Image => image_mime(&data)
                .context("the service sent something that is not an image")?
                .to_owned(),
            MediaKind::Video if media_type.starts_with("video/") => media_type,
            MediaKind::Video => bail!("the service sent {media_type}, not a video"),
            MediaKind::Model3d => GLB.to_owned(),
        };
        let name = file_name(call, &mime);
        let id = self
            .media
            .user
            .save_file(self.media.chat, &name, &mime, &data)
            .await
            .context("could not keep it with the chat")?;
        let file = json!({ "id": id, "name": name, "mime": mime, "size": data.len() });
        let saved = match &self.media.workspace {
            Some(workspace) => workspace
                .save(&name, &data)
                .await
                .map(|saved| saved.path)
                .map_err(|error| format!("{error:#}")),
            None => Err("the user has no environment online".to_owned()),
        };
        let mut output = match self.kind {
            MediaKind::Image => json!({
                "action": "send_image",
                "images": [{ "file": file, "caption": "" }],
            }),
            _ => json!({ "action": "send_file", "file": file }),
        };
        output["provider"] = json!(call.provider.name());
        output["model"] = json!(call.model);
        match saved {
            Ok(path) => output["path"] = json!(path),
            Err(why) => {
                output["note"] = json!(format!(
                    "Shown to the user, but not saved in the home folder: {why}"
                ));
            }
        }
        Ok(output)
    }
}

/// A binary glTF model's media type.
const GLB: &str = "model/gltf-binary";

fn job_error(error: JobError) -> anyhow::Error {
    anyhow::Error::msg(error.to_string())
}

/// The file name: `filename` or the prompt's first words, plus a short unique tag.
pub(super) fn file_name(call: &Call, mime: &str) -> String {
    let extension = match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "video/webm" => "webm",
        "video/quicktime" => "mov",
        GLB => "glb",
        _ if mime.starts_with("video/") => "mp4",
        _ => "bin",
    };
    let stem = match &call.filename {
        Some(name) => {
            let name = clean_name(name);
            name.rsplit_once('.')
                .filter(|(_, ext)| ext.eq_ignore_ascii_case(extension))
                .map_or(name.clone(), |(stem, _)| stem.to_owned())
        }
        None => {
            let words: Vec<String> = call
                .prompt
                .split(|c: char| !c.is_alphanumeric())
                .filter(|word| !word.is_empty())
                .take(NAME_WORDS)
                .map(str::to_lowercase)
                .collect();
            let fallback = match call.kind {
                MediaKind::Image => "image",
                MediaKind::Video => "video",
                MediaKind::Model3d => "model",
            };
            let stem = if words.is_empty() {
                fallback.to_owned()
            } else {
                words.join("-")
            };
            let tag = Uuid::new_v4().simple().to_string();
            format!("{stem}-{}", &tag[..6])
        }
    };
    format!("{stem}.{extension}")
}
