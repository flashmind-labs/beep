//! Asking each service for an image, a video or a 3D model. OpenRouter and
//! Venice go through flashmind's providers, Meshy through its 3D client;
//! OpenAI's image generation is done here, as flashmind's sends a field
//! GPT image models refuse.

use super::{
    call::Call,
    job::{JobError, Produced, bounded, first_file},
    openai,
    provider::{MediaKind, MediaProvider},
};
use anyhow::{Context, Result, bail};
use flashmind::{
    llm::{
        OpenAiCompatProfile, OpenAiProvider, OpenRouterProvider,
        three_d::{ThreeDClient, ThreeDImage, ThreeDInput, ThreeDProvider, ThreeDRequest},
    },
    types::{
        CompletionStream, FrameImage, ImageEditRequest, ImageGenRequest, LlmProvider, Model,
        VideoGenRequest,
    },
};
use reqwest::Url;
use secrecy::{ExposeSecret, SecretString};
use serde_json::Map;
use tokio_util::sync::CancellationToken;

/// Where each service's API is. Only tests point them elsewhere; OpenRouter
/// has no override.
#[derive(Debug, Clone)]
pub(crate) struct Endpoints {
    pub(super) openai: Url,
    pub(super) venice: Url,
    pub(super) meshy: Url,
}

impl Endpoints {
    pub(crate) fn public() -> Result<Self> {
        Ok(Self {
            openai: Url::parse("https://api.openai.com/v1")?,
            venice: Url::parse("https://api.venice.ai/api/v1")?,
            meshy: Url::parse("https://api.meshy.ai/")?,
        })
    }
}

/// One job: a checked call, with its input image as a URL or data URI.
pub(super) struct Job<'a> {
    pub(super) call: &'a Call,
    pub(super) image: Option<String>,
    pub(super) key: &'a SecretString,
    pub(super) endpoints: &'a Endpoints,
}

/// The image or video `job` asks for. Polling lines go to `progress`.
pub(super) async fn media(
    job: Job<'_>,
    cancel: &CancellationToken,
    progress: &mut (dyn FnMut(&str) + Send),
) -> Result<Produced, JobError> {
    let stream = match job.call.kind {
        MediaKind::Image => image_stream(&job),
        MediaKind::Video => video_stream(&job),
        MediaKind::Model3d => Err(anyhow::anyhow!("a 3D model is not a stream")),
    }
    .map_err(JobError::Failed)?;
    first_file(stream, job.call.kind.limit(), cancel, progress).await
}

fn openai_compatible(
    base: &Url,
    profile: OpenAiCompatProfile,
    key: &SecretString,
) -> Result<OpenAiProvider> {
    OpenAiProvider::builder(base.as_str())
        .profile(profile)
        .api_key(key.expose_secret())
        .build()
}

fn openrouter(key: &SecretString) -> OpenRouterProvider {
    OpenRouterProvider::new(key.expose_secret().to_owned()).with_app_name("Beep".into())
}

/// The image request flashmind's providers take.
pub(super) fn image_request(job: &Job<'_>) -> ImageGenRequest {
    let call = job.call;
    ImageGenRequest {
        model: call.model.clone(),
        prompt: call.prompt.clone(),
        size: call.size.clone(),
        aspect_ratio: call.aspect_ratio.clone(),
        quality: None,
        style: None,
        n: Some(1),
        input_references: job.image.iter().cloned().collect(),
        output_format: None,
    }
}

/// The edit request flashmind's providers take.
pub(super) fn edit_request(job: &Job<'_>, image: &str) -> ImageEditRequest {
    let call = job.call;
    ImageEditRequest {
        model: call.model.clone(),
        prompt: call.prompt.clone(),
        images: vec![image.to_owned()],
        resolution: call.size.clone(),
        // OpenAI's edits take a size and refuse an aspect ratio.
        aspect_ratio: call
            .aspect_ratio
            .clone()
            .filter(|_| call.provider != MediaProvider::OpenAi),
        quality: None,
        mask: None,
        output_format: None,
    }
}

fn image_stream(job: &Job<'_>) -> Result<CompletionStream> {
    let call = job.call;
    Ok(match (call.provider, job.image.as_deref()) {
        // OpenRouter takes the image as a reference on the same endpoint.
        (MediaProvider::OpenRouter, _) => openrouter(job.key).generate_image(image_request(job)),
        (MediaProvider::OpenAi, None) => openai::generate(
            &job.endpoints.openai,
            job.key.clone(),
            openai::ImageBody::new(call),
        ),
        (MediaProvider::OpenAi, Some(image)) => {
            openai_compatible(&job.endpoints.openai, OpenAiCompatProfile::OpenAi, job.key)?
                .edit_image(edit_request(job, image))
        }
        (MediaProvider::Venice, None) => {
            openai_compatible(&job.endpoints.venice, OpenAiCompatProfile::Venice, job.key)?
                .generate_image(image_request(job))
        }
        (MediaProvider::Venice, Some(image)) => {
            openai_compatible(&job.endpoints.venice, OpenAiCompatProfile::Venice, job.key)?
                .edit_image(edit_request(job, image))
        }
        (MediaProvider::Meshy, _) => bail!("Meshy does not make images"),
    })
}

/// Venice takes a video's length as required.
const VENICE_SECONDS: u32 = 5;

/// The video request flashmind's providers take.
pub(super) fn video_request(job: &Job<'_>) -> VideoGenRequest {
    let call = job.call;
    let seconds = match call.provider {
        MediaProvider::Venice => Some(call.seconds.unwrap_or(VENICE_SECONDS)),
        _ => call.seconds,
    };
    VideoGenRequest {
        model: call.model.clone(),
        description: call.prompt.clone(),
        resolution: call.resolution.clone(),
        aspect_ratio: call.aspect_ratio.clone(),
        duration: seconds,
        generate_audio: None,
        frame_images: job
            .image
            .iter()
            .map(|url| FrameImage {
                url: url.clone(),
                frame_type: "first_frame".to_owned(),
            })
            .collect(),
        input_references: Vec::new(),
        video_references: Vec::new(),
    }
}

fn video_stream(job: &Job<'_>) -> Result<CompletionStream> {
    Ok(match job.call.provider {
        MediaProvider::OpenRouter => openrouter(job.key).generate_video(video_request(job)),
        MediaProvider::Venice => {
            openai_compatible(&job.endpoints.venice, OpenAiCompatProfile::Venice, job.key)?
                .generate_video(video_request(job))
        }
        other => bail!("{} does not make videos", other.name()),
    })
}

/// Makes the 3D model `job` asks for and returns where its GLB is. Each
/// task Meshy starts is told to `progress`.
pub(super) async fn model_3d(
    job: Job<'_>,
    cancel: &CancellationToken,
    progress: &mut (dyn FnMut(&str) + Send),
) -> Result<Url, JobError> {
    let call = job.call;
    if call.provider != MediaProvider::Meshy {
        return Err(JobError::Failed(anyhow::anyhow!(
            "{} does not make 3D models",
            call.provider.name()
        )));
    }
    let request = model_request(&job).map_err(JobError::Failed)?;
    let client = ThreeDClient::new(ThreeDProvider::Meshy, job.key.clone())
        .map_err(JobError::Failed)?
        .with_base_url(job.endpoints.meshy.clone());
    let textured = call.texture && job.image.is_none();
    let mut tasks = 0;
    let on_task = |_: &str| {
        tasks += 1;
        progress(match (tasks, textured) {
            (1, _) => "Meshy is shaping the model",
            _ => "Meshy is painting the textures",
        });
    };
    bounded(client.generate(request, on_task), call.kind.limit(), cancel).await
}

/// The request Meshy's client takes: the image when there is one, else
/// the prompt.
pub(super) fn model_request(job: &Job<'_>) -> Result<ThreeDRequest> {
    let call = job.call;
    let model: Model = format!("meshy:{}", call.model)
        .parse()
        .context("not a Meshy model id")?;
    let input = match &job.image {
        Some(image) => ThreeDInput::Images(vec![ThreeDImage::Url(
            Url::parse(image).context("the image is not a URL")?,
        )]),
        None => ThreeDInput::Text(call.prompt.clone()),
    };
    Ok(ThreeDRequest {
        model,
        input,
        texture: call.texture,
        extra_args: Map::new(),
    })
}
