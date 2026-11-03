//! A media call's arguments, checked, and how its approval card shows them.

use super::provider::{MediaKind, MediaProvider};
use crate::{permissions::shorten, protocol::ApprovalField, tools::args::text};
use serde_json::{Value, json};
use std::str::FromStr;

/// Longest prompt taken.
const MAX_PROMPT: usize = 4000;
/// Longest value a card shows.
const MAX_SHOWN: usize = 600;
/// Longest video asked for, in seconds.
const MAX_SECONDS: u64 = 30;

/// Where an input image comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ImageSource {
    /// A public https URL the service fetches itself.
    Url(String),
    /// A file in the user's home folder.
    Path(String),
}

/// One call, checked.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Call {
    pub(super) kind: MediaKind,
    pub(super) provider: MediaProvider,
    pub(super) model: String,
    /// Empty only for a 3D model made from an image alone.
    pub(super) prompt: String,
    /// The image to edit, the video's first frame, or the picture to model.
    pub(super) image: Option<ImageSource>,
    /// An image's pixel size, such as `1024x1536`.
    pub(super) size: Option<String>,
    pub(super) aspect_ratio: Option<String>,
    /// A video's resolution, such as `720p`.
    pub(super) resolution: Option<String>,
    /// A video's length in seconds.
    pub(super) seconds: Option<u32>,
    /// Whether a 3D model is textured.
    pub(super) texture: bool,
    /// What the file is called, without its extension.
    pub(super) filename: Option<String>,
}

impl Call {
    /// Reads `args` for a `kind` tool; the provider must be among `available`.
    pub(super) fn parse(
        kind: MediaKind,
        available: &[MediaProvider],
        args: &Value,
    ) -> Result<Self, String> {
        let provider = match text(args, "provider") {
            Some(slug) => MediaProvider::from_str(slug)
                .ok()
                .filter(|provider| available.contains(provider))
                .ok_or_else(|| {
                    format!(
                        "No key for {slug}. Use one of: {}.",
                        slugs(available).join(", ")
                    )
                })?,
            None => *available
                .first()
                .ok_or("The user has no key for a service that makes this.")?,
        };
        let image = match text(args, "image") {
            Some(url) if url.starts_with("https://") => Some(ImageSource::Url(url.to_owned())),
            Some(url) if url.contains("://") => {
                return Err("`image` must be an https URL or a file in the home folder.".into());
            }
            Some(path) => Some(ImageSource::Path(path.to_owned())),
            None => None,
        };
        let prompt = text(args, "prompt").unwrap_or_default();
        if prompt.chars().count() > MAX_PROMPT {
            return Err(format!(
                "The prompt is longer than {MAX_PROMPT} characters."
            ));
        }
        if prompt.is_empty() && !(kind == MediaKind::Model3d && image.is_some()) {
            return Err(match kind {
                MediaKind::Model3d => "Give `prompt`, `image` or both.".into(),
                _ => "Give `prompt`, what to make.".into(),
            });
        }
        let model = match text(args, "model") {
            Some(model) => model.to_owned(),
            None => kind
                .default_model(provider, image.is_some())
                .ok_or_else(|| format!("{} does not make this.", provider.name()))?
                .to_owned(),
        };
        let seconds = match args.get("duration").filter(|value| !value.is_null()) {
            Some(value) => {
                let seconds = value
                    .as_u64()
                    .filter(|seconds| (1..=MAX_SECONDS).contains(seconds))
                    .ok_or(format!("`duration` is whole seconds, 1 to {MAX_SECONDS}."))?;
                Some(u32::try_from(seconds).map_err(|e| e.to_string())?)
            }
            None => None,
        };
        Ok(Self {
            kind,
            provider,
            model,
            prompt: prompt.to_owned(),
            image,
            size: text(args, "size").map(str::to_owned),
            aspect_ratio: text(args, "aspect_ratio").map(str::to_owned),
            resolution: text(args, "resolution").map(str::to_owned),
            seconds,
            texture: args.get("texture").and_then(Value::as_bool).unwrap_or(true),
            filename: text(args, "filename").map(str::to_owned),
        })
    }
}

fn slugs(providers: &[MediaProvider]) -> Vec<&'static str> {
    providers.iter().map(|provider| provider.slug()).collect()
}

/// The tool's arguments, with `provider` one of `available`.
pub(super) fn schema(kind: MediaKind, available: &[MediaProvider]) -> Value {
    let mut properties = json!({
        "prompt": {
            "type": "string",
            "description": match kind {
                MediaKind::Image => "What the image shows, or for an edit what to change",
                MediaKind::Video => "What happens in the video: subject, action, camera, style",
                MediaKind::Model3d => "The object to model, such as 'a low-poly wooden chair'; optional with `image`",
            }
        },
        "provider": {
            "type": "string",
            "enum": slugs(available),
            "description": "The service that makes it, one the user has a key for"
        },
        "model": {
            "type": "string",
            "description": "The service's model id; leave out for its default"
        },
        "image": {
            "type": "string",
            "description": match kind {
                MediaKind::Image => "An image to edit or work from: an https URL, or a PNG, JPEG or WebP in the user's home folder",
                MediaKind::Video => "The video's first frame: an https URL, or a PNG, JPEG or WebP in the user's home folder",
                MediaKind::Model3d => "A picture of the object: an https URL, or a PNG, JPEG or WebP in the user's home folder",
            }
        },
        "filename": {
            "type": "string",
            "description": "Name to save it as in the home folder, without extension; taken from the prompt when left out"
        }
    });
    match kind {
        MediaKind::Image => {
            properties["size"] = json!({ "type": "string", "description": "Pixel size such as 1024x1024 or 1536x1024, where the service takes one" });
            properties["aspect_ratio"] =
                json!({ "type": "string", "description": "Such as 1:1, 16:9 or 9:16" });
        }
        MediaKind::Video => {
            properties["duration"] = json!({ "type": "integer", "minimum": 1, "maximum": MAX_SECONDS, "description": "Length in seconds; each model takes only some lengths, often 5 or 8" });
            properties["aspect_ratio"] =
                json!({ "type": "string", "description": "Such as 16:9 or 9:16" });
            properties["resolution"] =
                json!({ "type": "string", "description": "Such as 720p or 1080p" });
        }
        MediaKind::Model3d => {
            properties["texture"] = json!({ "type": "boolean", "description": "Paint textures on the model (default true); false is faster and cheaper" });
        }
    }
    let required: &[&str] = match kind {
        MediaKind::Model3d => &["provider"],
        _ => &["prompt", "provider"],
    };
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

/// What the approval card shows for a `kind` call: prompt, service, model, then the rest.
pub(crate) fn approval_fields(kind: MediaKind, args: &Value) -> Vec<ApprovalField> {
    let field = |label: &str, value: &str| ApprovalField {
        label: label.to_owned(),
        value: shorten(value, MAX_SHOWN),
    };
    let mut fields = Vec::new();
    if let Some(prompt) = text(args, "prompt") {
        fields.push(field("Prompt", prompt));
    }
    let provider = text(args, "provider").and_then(|slug| MediaProvider::from_str(slug).ok());
    if let Some(provider) = provider {
        fields.push(field("Provider", provider.name()));
    }
    let model = text(args, "model")
        .or_else(|| kind.default_model(provider?, text(args, "image").is_some()));
    if let Some(model) = model {
        fields.push(field("Model", model));
    }
    let image_label = match kind {
        MediaKind::Video => "Start image",
        _ => "Image",
    };
    for (key, label) in [
        ("image", image_label),
        ("size", "Size"),
        ("aspect_ratio", "Aspect ratio"),
        ("resolution", "Resolution"),
    ] {
        if let Some(value) = text(args, key) {
            fields.push(field(label, value));
        }
    }
    if let Some(seconds) = args.get("duration").and_then(Value::as_u64) {
        fields.push(field("Duration", &format!("{seconds} s")));
    }
    if kind == MediaKind::Model3d {
        let texture = args.get("texture").and_then(Value::as_bool).unwrap_or(true);
        fields.push(field("Texture", if texture { "Yes" } else { "No" }));
    }
    fields
}
