//! The OpenRouter model catalog, the default pick and the providers of a model. The catalog comes
//! from flashmind's OpenRouter provider, which already knows capabilities, prices and the image,
//! video and audio models.

use crate::{AssistantError, Endpoint, openrouter::check};
use flashmind_llm::{LlmProvider, OpenRouterProvider};
use flashmind_types::{ModelCategory, ModelInfo as CatalogEntry};
use reqwest::Client;
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use std::{future::Future, time::Duration};
use strum::VariantArray;
use tokio::time::timeout;

/// How long one listing may take.
const FETCH_TIMEOUT: Duration = Duration::from_secs(15);

/// What a model can do, in the order and with the labels of the flashmind CLI's
/// `CATALOG_FEATURES` (`flashmind-cli/src/setup.rs`).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    strum::IntoStaticStr,
    strum::VariantArray,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum Feature {
    Chat,
    Tools,
    Reasoning,
    Vision,
    Documents,
    AudioInput,
    VideoInput,
    Image,
    Video,
    ImageToVideo,
    Speech,
    Transcribe,
    ImageEdit,
    StyleReference,
}

impl Feature {
    /// Every feature, in the CLI's order.
    pub fn all() -> &'static [Feature] {
        Feature::VARIANTS
    }
}

/// The features of a catalog entry. A port of `model_features` in `flashmind-cli/src/setup.rs`,
/// which is private to that binary; it should move into `flashmind-types` so both share it.
fn features(model: &CatalogEntry) -> Vec<Feature> {
    let c = &model.capabilities;
    let has = |category| model.categories.contains(&category);
    [
        (has(ModelCategory::Chat), Feature::Chat),
        (c.tool_calling, Feature::Tools),
        (
            c.reasoning || has(ModelCategory::Reasoning),
            Feature::Reasoning,
        ),
        (c.images || has(ModelCategory::Vision), Feature::Vision),
        (c.documents, Feature::Documents),
        (c.audio, Feature::AudioInput),
        (
            c.video || has(ModelCategory::VideoInput),
            Feature::VideoInput,
        ),
        (
            c.image_generation || has(ModelCategory::ImageGeneration),
            Feature::Image,
        ),
        (
            c.video_generation || has(ModelCategory::VideoGeneration),
            Feature::Video,
        ),
        (has(ModelCategory::ImageToVideo), Feature::ImageToVideo),
        (c.audio_output || has(ModelCategory::Tts), Feature::Speech),
        (has(ModelCategory::Stt), Feature::Transcribe),
        (
            model.image.editing || has(ModelCategory::ImageEditing),
            Feature::ImageEdit,
        ),
        (
            model.image.style_references == Some(true),
            Feature::StyleReference,
        ),
    ]
    .into_iter()
    .filter_map(|(enabled, feature)| enabled.then_some(feature))
    .collect()
}

/// A model the picker offers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    /// Seconds since the epoch, when the catalog says.
    pub created: Option<i64>,
    pub context_length: Option<u32>,
    pub features: Vec<Feature>,
    /// It takes tool definitions.
    pub tools: bool,
    /// It reads pictures.
    pub images: bool,
    /// USD per million tokens, when listed.
    pub input_per_m: Option<f64>,
    pub output_per_m: Option<f64>,
}

/// A price per token, per million tokens.
fn per_million<D>(price: Option<D>) -> Option<f64>
where
    f64: TryFrom<D>,
{
    f64::try_from(price?).ok().map(|per_token| per_token * 1e6)
}

fn summarize(entry: CatalogEntry) -> ModelInfo {
    let features = features(&entry);
    ModelInfo {
        name: entry.name.clone().unwrap_or_else(|| entry.id.clone()),
        created: entry.created_at.map(|at| at.timestamp()),
        context_length: entry.context_length,
        tools: features.contains(&Feature::Tools),
        images: features.contains(&Feature::Vision),
        input_per_m: per_million(entry.pricing.prompt),
        output_per_m: per_million(entry.pricing.completion),
        features,
        id: entry.id,
    }
}

/// Adds the audio models to the catalog, as the CLI's `models` command does: a model that is
/// already listed gains the category, any other is added with it.
fn add_audio(
    models: &mut Vec<CatalogEntry>,
    entries: Vec<(String, Option<String>)>,
    category: ModelCategory,
) {
    for (id, name) in entries {
        if let Some(model) = models.iter_mut().find(|model| model.id == id) {
            if !model.categories.contains(&category) {
                model.categories.push(category);
            }
        } else {
            models.push(CatalogEntry {
                image: Default::default(),
                id,
                name,
                created_at: None,
                context_length: None,
                max_completion_tokens: None,
                capabilities: Default::default(),
                categories: vec![category],
                pricing: Default::default(),
            });
        }
    }
}

/// A listing that took too long or failed, as the user reads it. A failed audio listing only
/// costs the audio models.
async fn bounded<T, E: std::fmt::Display>(
    what: &str,
    listing: impl Future<Output = Result<T, E>>,
) -> Result<T, AssistantError> {
    match timeout(FETCH_TIMEOUT, listing).await {
        Ok(Ok(listed)) => Ok(listed),
        Ok(Err(e)) => Err(AssistantError::Api(format!("Could not list {what}: {e}"))),
        Err(_) => Err(AssistantError::Api(format!("Listing {what} timed out."))),
    }
}

/// The models with `feature` (all of them with none), newest first. The audio models are fetched
/// only when the feature can ask for them.
pub async fn list_models(
    endpoint: &Endpoint,
    feature: Option<Feature>,
) -> Result<Vec<ModelInfo>, AssistantError> {
    let provider = OpenRouterProvider::new(endpoint.key.expose_secret().to_owned());
    let mut models = timeout(FETCH_TIMEOUT, provider.list_models())
        .await
        .map_err(|_| AssistantError::Api("Listing models timed out.".into()))?
        .ok_or_else(|| AssistantError::Api("OpenRouter would not list its models.".into()))?;
    if matches!(
        feature,
        None | Some(Feature::Speech) | Some(Feature::Transcribe)
    ) {
        let speech = bounded("speech models", provider.list_speech_models()).await;
        let transcription =
            bounded("transcription models", provider.list_transcription_models()).await;
        for (audio, category) in [
            (speech, ModelCategory::Tts),
            (transcription, ModelCategory::Stt),
        ] {
            match audio {
                Ok(entries) => add_audio(
                    &mut models,
                    entries.into_iter().map(|m| (m.id, m.name)).collect(),
                    category,
                ),
                Err(e) => log::warn!("{e}"),
            }
        }
    }
    let mut listed: Vec<ModelInfo> = models
        .into_iter()
        .map(summarize)
        .filter(|model| feature.is_none_or(|f| model.features.contains(&f)))
        .collect();
    listed.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| a.id.cmp(&b.id)));
    Ok(listed)
}

/// An upstream provider that serves a model.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Provider {
    /// What `provider.order` takes, such as `deepinfra/turbo`.
    pub tag: String,
    pub name: String,
}

#[derive(Deserialize)]
struct EndpointsReply {
    data: EndpointsData,
}

#[derive(Deserialize)]
struct EndpointsData {
    #[serde(default)]
    endpoints: Vec<RawEndpoint>,
}

#[derive(Deserialize)]
struct RawEndpoint {
    tag: Option<String>,
    provider_name: Option<String>,
    name: Option<String>,
}

fn parse_providers(body: &[u8]) -> Result<Vec<Provider>, AssistantError> {
    let reply: EndpointsReply = serde_json::from_slice(body)?;
    let mut providers: Vec<Provider> = Vec::new();
    for endpoint in reply.data.endpoints {
        let Some(tag) = endpoint.tag.filter(|tag| !tag.is_empty()) else {
            continue;
        };
        if providers.iter().any(|p| p.tag == tag) {
            continue;
        }
        let name = endpoint
            .provider_name
            .or(endpoint.name)
            .unwrap_or_else(|| tag.clone());
        providers.push(Provider { tag, name });
    }
    Ok(providers)
}

/// The upstream providers OpenRouter routes `model` (`author/slug`) to.
pub async fn list_providers(
    client: &Client,
    endpoint: &Endpoint,
    model: &str,
) -> Result<Vec<Provider>, AssistantError> {
    let response = client
        .get(format!("{}/models/{model}/endpoints", endpoint.base))
        .bearer_auth(endpoint.key.expose_secret())
        .send()
        .await?;
    let body = check(response).await?.bytes().await?;
    parse_providers(&body)
}

/// Families to prefer when nothing was used before. They are prefixes matched against the live
/// list, so a family that is gone is skipped.
const PREFERRED: &[&str] = &[
    "anthropic/claude-sonnet",
    "openai/gpt-5",
    "google/gemini-2.5-pro",
];

/// The model a new chat starts on: the one used last if it is still listed, else the first model
/// of a preferred family that takes tools, else the first that takes tools, else the first.
pub fn default_model(models: &[ModelInfo], last_used: Option<&str>) -> Option<String> {
    if let Some(last) = last_used.filter(|last| models.iter().any(|m| m.id == *last)) {
        return Some(last.to_owned());
    }
    let preferred = PREFERRED
        .iter()
        .find_map(|prefix| models.iter().find(|m| m.tools && m.id.starts_with(prefix)));
    preferred
        .or_else(|| models.iter().find(|m| m.tools))
        .or(models.first())
        .map(|m| m.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str, tools: bool) -> ModelInfo {
        ModelInfo {
            id: id.into(),
            name: id.into(),
            created: None,
            context_length: None,
            features: Vec::new(),
            tools,
            images: false,
            input_per_m: None,
            output_per_m: None,
        }
    }

    fn entry(id: &str) -> CatalogEntry {
        CatalogEntry {
            id: id.into(),
            name: None,
            created_at: None,
            context_length: Some(8000),
            max_completion_tokens: None,
            capabilities: Default::default(),
            categories: Vec::new(),
            pricing: Default::default(),
            image: Default::default(),
        }
    }

    #[test]
    fn features_follow_the_cli_order() {
        let mut model = entry("a/b");
        model.categories = vec![ModelCategory::Chat, ModelCategory::ImageToVideo];
        model.capabilities.tool_calling = true;
        model.capabilities.images = true;
        assert_eq!(
            features(&model),
            [
                Feature::Chat,
                Feature::Tools,
                Feature::Vision,
                Feature::ImageToVideo
            ]
        );
        assert_eq!(<&str>::from(Feature::ImageToVideo), "image-to-video");
        assert_eq!(Feature::all().len(), 14);
        assert_eq!(Feature::all()[0], Feature::Chat);
    }

    #[test]
    fn a_summary_prices_per_million_and_marks_features() {
        let mut model = entry("a/b");
        model.categories = vec![ModelCategory::Chat];
        model.capabilities.tool_calling = true;
        model.pricing.prompt = Some("0.000003".parse().unwrap());
        let info = summarize(model);
        assert_eq!(info.name, "a/b");
        assert!(info.tools && !info.images);
        assert_eq!(info.input_per_m.map(|p| (p * 100.0).round()), Some(300.0));
        assert_eq!(info.output_per_m, None);
        assert_eq!(info.context_length, Some(8000));
    }

    #[test]
    fn audio_models_join_the_catalog() {
        let mut models = vec![entry("a/listed")];
        add_audio(
            &mut models,
            vec![
                ("a/listed".into(), None),
                ("b/voice".into(), Some("Voice".into())),
            ],
            ModelCategory::Tts,
        );
        assert_eq!(models.len(), 2);
        assert!(features(&models[0]).contains(&Feature::Speech));
        assert_eq!(features(&models[1]), [Feature::Speech]);
        assert_eq!(models[1].name.as_deref(), Some("Voice"));
    }

    #[test]
    fn providers_parse_and_repeat_tags_collapse() {
        let body = br#"{"data":{"id":"a/b","endpoints":[
            {"name":"A: B","provider_name":"DeepInfra","tag":"deepinfra/turbo"},
            {"name":"A: B","provider_name":"DeepInfra","tag":"deepinfra/turbo"},
            {"name":"A: B","provider_name":"Other","tag":"other"},
            {"name":"no tag","provider_name":"Skipped"}]}}"#;
        let providers = parse_providers(body).unwrap();
        assert_eq!(
            providers,
            [
                Provider {
                    tag: "deepinfra/turbo".into(),
                    name: "DeepInfra".into()
                },
                Provider {
                    tag: "other".into(),
                    name: "Other".into()
                }
            ]
        );
        assert!(parse_providers(b"{}").is_err());
    }

    #[test]
    fn the_last_used_model_wins_while_listed() {
        let list = [model("a/one", true), model("b/two", true)];
        assert_eq!(
            default_model(&list, Some("b/two")).as_deref(),
            Some("b/two")
        );
        assert_eq!(
            default_model(&list, Some("gone/x")).as_deref(),
            Some("a/one")
        );
    }

    #[test]
    fn a_preferred_family_comes_first_and_absence_falls_back() {
        let list = [model("a/one", true), model("openai/gpt-5-mini", true)];
        assert_eq!(
            default_model(&list, None).as_deref(),
            Some("openai/gpt-5-mini")
        );
        let list = [model("a/plain", false), model("b/tools", true)];
        assert_eq!(default_model(&list, None).as_deref(), Some("b/tools"));
        assert_eq!(
            default_model(&[model("c/only", false)], None).as_deref(),
            Some("c/only")
        );
        assert_eq!(default_model(&[], None), None);
    }
}
