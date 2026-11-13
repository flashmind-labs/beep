//! flsh's own settings on the Coding agents page: OpenRouter routing and
//! auto-compact, the same on every machine. The page sends plain text; it
//! is parsed into the protocol's types here, and checked before it goes.

use crate::{
    error::{DesktopError, Result},
    fleet::projects::{FleetState, desk},
};
use beep_proto::{
    FlshAutoCompact, FlshModel, FlshModelRule, FlshQuantization, FlshSettings, FlshSort,
};
use serde::{Deserialize, Serialize};

/// The settings as the page reads and writes them. Null or empty means
/// flsh's own default.
#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct FlshView {
    zdr: Option<bool>,
    quantizations: Vec<String>,
    /// `price`, `throughput` or `latency`.
    sort: Option<String>,
    allow_fallbacks: Option<bool>,
    autocompact: Option<FlshAutoCompact>,
    models: Vec<ModelRuleView>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ModelRuleView {
    /// `provider:model`, such as `openrouter:vendor/model`.
    model: String,
    enabled: bool,
    tokens: Option<u32>,
}

impl From<FlshSettings> for FlshView {
    fn from(settings: FlshSettings) -> Self {
        Self {
            zdr: settings.zdr,
            quantizations: settings
                .quantizations
                .into_iter()
                .map(|q| <&str>::from(q).to_owned())
                .collect(),
            sort: settings.sort.map(|sort| <&str>::from(sort).to_owned()),
            allow_fallbacks: settings.allow_fallbacks,
            autocompact: settings.autocompact,
            models: settings
                .models
                .into_iter()
                .map(|rule| ModelRuleView {
                    model: rule.model.to_string(),
                    enabled: rule.autocompact.enabled,
                    tokens: rule.autocompact.tokens,
                })
                .collect(),
        }
    }
}

impl TryFrom<FlshView> for FlshSettings {
    type Error = DesktopError;

    fn try_from(view: FlshView) -> Result<Self> {
        let bad = |text: String| DesktopError::BadRequest(text);
        let quantizations = view
            .quantizations
            .iter()
            .map(|q| {
                q.parse::<FlshQuantization>()
                    .map_err(|_| bad(format!("{q} is not a quantization OpenRouter knows.")))
            })
            .collect::<Result<Vec<_>>>()?;
        let sort = view
            .sort
            .as_deref()
            .map(|sort| {
                sort.parse::<FlshSort>()
                    .map_err(|_| bad(format!("{sort} is not a provider sort.")))
            })
            .transpose()?;
        let models = view
            .models
            .into_iter()
            .map(|rule| {
                let name = rule.model.trim();
                let model = name.parse::<FlshModel>().map_err(|_| {
                    bad(format!(
                        "{name} is not a model flsh knows; name it provider:model, such as openrouter:vendor/model."
                    ))
                })?;
                Ok(FlshModelRule {
                    model,
                    autocompact: FlshAutoCompact {
                        enabled: rule.enabled,
                        tokens: rule.tokens,
                    },
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let settings = FlshSettings {
            zdr: view.zdr,
            quantizations,
            sort,
            allow_fallbacks: view.allow_fallbacks,
            autocompact: view.autocompact,
            models,
        };
        settings.check().map_err(|e| bad(e.to_string()))?;
        Ok(settings)
    }
}

/// The user's flsh settings.
#[tauri::command]
pub async fn flsh_settings_get(fleet: FleetState<'_>) -> Result<Option<FlshView>> {
    Ok(Some(desk(&fleet).await?.flsh().into()))
}

/// Saves the user's flsh settings for every machine.
#[tauri::command]
pub async fn flsh_settings_save(fleet: FleetState<'_>, settings: FlshView) -> Result<()> {
    let settings = FlshSettings::try_from(settings)?;
    Ok(desk(&fleet).await?.save_flsh(&settings).await?)
}

#[cfg(test)]
#[path = "../tests/agents_flsh_settings.rs"]
mod tests;
