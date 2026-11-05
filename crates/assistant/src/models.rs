//! The OpenRouter model list and the default pick.

use crate::{AssistantError, Endpoint, openrouter::check};
use reqwest::Client;
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};

/// A model the picker offers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    /// It takes tool definitions.
    pub tools: bool,
    /// It reads pictures.
    pub images: bool,
    /// USD per million tokens, when listed.
    pub input_per_m: Option<f64>,
    pub output_per_m: Option<f64>,
}

#[derive(Deserialize)]
struct Listing {
    #[serde(default)]
    data: Vec<Raw>,
}

#[derive(Deserialize)]
struct Raw {
    id: String,
    name: Option<String>,
    supported_parameters: Option<Vec<String>>,
    pricing: Option<Pricing>,
    architecture: Option<Architecture>,
}

#[derive(Deserialize)]
struct Pricing {
    prompt: Option<String>,
    completion: Option<String>,
}

#[derive(Deserialize)]
struct Architecture {
    input_modalities: Option<Vec<String>>,
}

/// A price per token as the listing writes it, per million tokens.
fn per_million(price: Option<&String>) -> Option<f64> {
    price?.parse::<f64>().ok().map(|per_token| per_token * 1e6)
}

/// Every model OpenRouter lists, in its order.
pub async fn list_models(
    client: &Client,
    endpoint: &Endpoint,
) -> Result<Vec<ModelInfo>, AssistantError> {
    let response = client
        .get(format!("{}/models", endpoint.base))
        .bearer_auth(endpoint.key.expose_secret())
        .send()
        .await?;
    let listing: Listing = check(response).await?.json().await?;
    Ok(listing
        .data
        .into_iter()
        .map(|raw| ModelInfo {
            name: raw.name.unwrap_or_else(|| raw.id.clone()),
            tools: raw
                .supported_parameters
                .is_none_or(|parameters| parameters.iter().any(|p| p == "tools")),
            images: raw
                .architecture
                .and_then(|a| a.input_modalities)
                .is_some_and(|m| m.iter().any(|m| m == "image")),
            input_per_m: per_million(raw.pricing.as_ref().and_then(|p| p.prompt.as_ref())),
            output_per_m: per_million(raw.pricing.as_ref().and_then(|p| p.completion.as_ref())),
            id: raw.id,
        })
        .collect())
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
            tools,
            images: false,
            input_per_m: None,
            output_per_m: None,
        }
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
