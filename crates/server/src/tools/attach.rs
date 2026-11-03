//! `attach`: shows the model a file from the user's home folder or machines.

use super::{
    file::{UserFiles, read_user_file},
    machines::Machines,
};
use crate::{fetch::MAX_DOWNLOAD, images::Describer, workspace::Workspace};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use flashmind::types::{
    ContentPart, ModelCapabilities,
    tool::{Tool, ToolContext, ToolResult},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

/// The tool's name, which the apps label.
pub(crate) const NAME: &str = "attach";

pub(crate) struct AttachTool {
    pub(crate) workspace: Option<Arc<Workspace>>,
    /// The user's machines, when this turn may use them.
    pub(crate) machines: Option<Machines>,
    /// Describes an image for a model that cannot see it.
    pub(crate) describer: Option<Describer>,
}

#[derive(Deserialize)]
struct Args {
    path: String,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    media_type: Option<String>,
}

/// What kind of part a file goes to the model as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Image,
    Document,
    Video,
    Audio,
}

impl Kind {
    fn of(media_type: &str) -> Self {
        if media_type.starts_with("image/") {
            Kind::Image
        } else if media_type.starts_with("video/") {
            Kind::Video
        } else if media_type.starts_with("audio/") {
            Kind::Audio
        } else {
            Kind::Document
        }
    }

    fn accepted(self, caps: &ModelCapabilities) -> bool {
        match self {
            Kind::Image => caps.images,
            Kind::Document => caps.documents,
            Kind::Video => caps.video,
            Kind::Audio => caps.audio,
        }
    }
}

/// The file as a part for the model.
struct Attached {
    kind: Kind,
    media_type: String,
    filename: String,
    data: Vec<u8>,
}

impl Attached {
    fn part(self) -> ContentPart {
        let Attached {
            kind,
            media_type,
            filename,
            data,
        } = self;
        let data = STANDARD.encode(data);
        match kind {
            Kind::Image => ContentPart::Image { media_type, data },
            Kind::Document => ContentPart::Document {
                media_type,
                filename,
                data,
            },
            Kind::Video => ContentPart::Video {
                media_type,
                filename,
                data,
            },
            Kind::Audio => ContentPart::Audio {
                media_type,
                filename,
                data,
            },
        }
    }
}

/// The media type of `path` by its extension.
fn media_type(path: &str) -> Option<&'static str> {
    let extension = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "pdf" => Some("application/pdf"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "mp4" => Some("video/mp4"),
        "webm" => Some("video/webm"),
        "mov" => Some("video/quicktime"),
        "mp3" => Some("audio/mpeg"),
        "wav" => Some("audio/wav"),
        "flac" => Some("audio/flac"),
        "ogg" => Some("audio/ogg"),
        "m4a" => Some("audio/mp4"),
        _ => None,
    }
}

/// The media type the call gave, else `path`'s; `None` when neither is a type.
fn chosen_type(given: Option<&str>, path: &str) -> Option<String> {
    given
        .map(str::trim)
        .filter(|given| !given.is_empty())
        .or_else(|| media_type(path))
        .filter(|mime| mime.contains('/'))
        .map(str::to_ascii_lowercase)
}

/// The file name at the end of `path`, for a label.
fn file_name(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
}

#[async_trait]
impl Tool for AttachTool {
    fn name(&self) -> &str {
        NAME
    }

    fn description(&self) -> &str {
        "Attach a PDF, image, audio or video file from the user's home folder or one of their \
         machines for you to examine, such as a scanned letter, a photo or a recording. Give \
         `path` (relative to the home folder, or absolute) and, for a file on another machine, \
         `from`. If this model cannot see images, a vision model describes them. Other formats \
         depend on model and provider support. Files up to 20 MiB. To give the user a file, use \
         send_file or send_image instead."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "The file, such as letter.pdf in the user's home folder; `~` is the home folder" },
                "from": { "type": "string", "description": "The machine `path` is on, by its name in the machines list; omit for the user's own environment" },
                "media_type": { "type": "string", "description": "MIME type, required for unrecognized file extensions" }
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }

    fn timeout_secs(&self) -> Option<u64> {
        Some(120)
    }

    fn humanize(&self, args: &Value) -> String {
        match args.get("path").and_then(Value::as_str) {
            Some(path) => format!("Looking at {}", file_name(path)),
            None => "Looking at a file".into(),
        }
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let call_id = ctx.tool_call_id;
        let args: Args = match serde_json::from_value(ctx.args.clone()) {
            Ok(args) => args,
            Err(error) => return Ok(ToolResult::failure(call_id, error.to_string())),
        };
        let Some(mime) = chosen_type(args.media_type.as_deref(), &args.path) else {
            return Ok(ToolResult::failure(
                call_id,
                "Unknown file type. Specify its MIME type using media_type.",
            ));
        };
        let (Some(llm), Some(provider)) = (ctx.llm, &ctx.provider) else {
            return Ok(ToolResult::failure(
                call_id,
                "Current model capabilities are unavailable.",
            ));
        };
        let kind = Kind::of(&mime);
        let accepted = kind.accepted(&provider.capabilities(&llm.model).await);
        let describer = match (accepted, kind, &self.describer) {
            (true, _, _) => None,
            (false, Kind::Image, Some(describer)) => Some(describer),
            (false, Kind::Image, None) => {
                return Ok(ToolResult::failure(
                    call_id,
                    format!(
                        "Model {} cannot see images, and no vision model is set up to describe \
                         them.",
                        llm.model
                    ),
                ));
            }
            (false, _, _) => {
                return Ok(ToolResult::failure(
                    call_id,
                    format!(
                        "Model {} does not report support for {mime} attachments.",
                        llm.model
                    ),
                ));
            }
        };
        let files = UserFiles {
            workspace: self.workspace.as_deref(),
            machines: self.machines.as_ref(),
        };
        let file = match read_user_file(files, args.from.as_deref(), &args.path).await {
            Ok(file) => file,
            Err(error) => {
                return Ok(ToolResult::failure(
                    call_id,
                    format!("Could not read the file: {error:#}"),
                ));
            }
        };
        // The read refuses a larger file; this holds it whatever the source.
        if file.data.len() as u64 > MAX_DOWNLOAD {
            return Ok(ToolResult::failure(
                call_id,
                format!("The file is larger than {} MiB.", MAX_DOWNLOAD >> 20),
            ));
        }
        if file.data.is_empty() {
            return Ok(ToolResult::failure(call_id, "The file is empty."));
        }
        let name = file.name;
        let attached = Attached {
            kind,
            media_type: mime,
            filename: name.clone(),
            data: file.data,
        };
        let Some(describer) = describer else {
            return Ok(
                ToolResult::success(call_id, format!("Attached file: {name}"))
                    .with_parts(vec![attached.part()]),
            );
        };
        Ok(
            match describer.describe_file(&attached.part(), &name).await {
                Ok(text) => ToolResult::success(
                    call_id,
                    format!(
                        "This model cannot see images, so a vision model described {name}:\n{text}"
                    ),
                ),
                Err(error) => {
                    ToolResult::failure(call_id, format!("Could not describe the image: {error:#}"))
                }
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_types_come_from_the_extension() {
        assert_eq!(media_type("scan.PDF"), Some("application/pdf"));
        assert_eq!(media_type("~/photos/cat.jpeg"), Some("image/jpeg"));
        assert_eq!(media_type("voice.m4a"), Some("audio/mp4"));
        assert_eq!(media_type("notes.txt"), None);
        assert_eq!(media_type("README"), None);
    }

    #[test]
    fn a_blank_given_type_falls_back_to_the_extension() {
        assert_eq!(
            chosen_type(Some(""), "letter.pdf").as_deref(),
            Some("application/pdf")
        );
        assert_eq!(
            chosen_type(Some(" Image/PNG "), "x").as_deref(),
            Some("image/png")
        );
        assert_eq!(chosen_type(Some("pdf"), "letter.pdf"), None);
        assert_eq!(chosen_type(None, "README"), None);
    }

    #[test]
    fn kinds_need_their_capability() {
        let images = ModelCapabilities {
            images: true,
            ..Default::default()
        };
        assert!(Kind::of("image/png").accepted(&images));
        assert!(!Kind::of("application/pdf").accepted(&images));
        assert!(!Kind::of("video/mp4").accepted(&images));
        assert_eq!(Kind::of("audio/mpeg"), Kind::Audio);
    }

    #[test]
    fn the_label_names_the_file() {
        let tool = AttachTool {
            workspace: None,
            machines: None,
            describer: None,
        };
        assert_eq!(
            tool.humanize(&json!({ "path": "/srv/project/scans/letter.pdf" })),
            "Looking at letter.pdf"
        );
        assert_eq!(tool.humanize(&json!({})), "Looking at a file");
    }
}
