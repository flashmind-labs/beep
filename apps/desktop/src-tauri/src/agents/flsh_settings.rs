//! flsh's own settings on the Coding agents page: OpenRouter routing, which
//! pi takes too, and auto-compact, for all models and per model, the same on
//! every machine. The page sends plain text; it
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
    /// Compacts as all models do; `enabled` and `tokens` are then unused.
    inherits_compaction: bool,
    enabled: bool,
    tokens: Option<u32>,
    /// Null keeps the list for all models; empty allows any.
    quantizations: Option<Vec<String>>,
}

impl From<FlshSettings> for FlshView {
    fn from(settings: FlshSettings) -> Self {
        Self {
            zdr: settings.zdr,
            quantizations: names(settings.quantizations),
            sort: settings.sort.map(|sort| <&str>::from(sort).to_owned()),
            allow_fallbacks: settings.allow_fallbacks,
            autocompact: settings.autocompact,
            models: settings
                .models
                .into_iter()
                .map(|rule| ModelRuleView {
                    model: rule.model.to_string(),
                    inherits_compaction: rule.inherits_compaction,
                    enabled: rule.autocompact.enabled,
                    tokens: rule.autocompact.tokens,
                    quantizations: rule.quantizations.map(names),
                })
                .collect(),
        }
    }
}

impl TryFrom<FlshView> for FlshSettings {
    type Error = DesktopError;

    fn try_from(view: FlshView) -> Result<Self> {
        let bad = |text: String| DesktopError::BadRequest(text);
        let quantizations = parsed(&view.quantizations)?;
        let sort = view
            .sort
            .as_deref()
            .map(|sort| {
                sort.parse::<FlshSort>()
                    .map_err(|_| bad(format!("{sort} is not a provider sort.")))
            })
            .transpose()?;
        // A released node writes a rule's compaction even when it inherits:
        // there it is the rule for all models.
        let inherited = view.autocompact.unwrap_or(FlshAutoCompact {
            enabled: true,
            tokens: None,
        });
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
                let autocompact = match rule.inherits_compaction {
                    true => inherited,
                    false => FlshAutoCompact {
                        enabled: rule.enabled,
                        tokens: rule.tokens,
                    },
                };
                Ok(FlshModelRule {
                    model,
                    autocompact,
                    inherits_compaction: rule.inherits_compaction,
                    quantizations: rule.quantizations.as_deref().map(parsed).transpose()?,
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

/// Quantizations as the page shows them.
fn names(quantizations: Vec<FlshQuantization>) -> Vec<String> {
    quantizations
        .into_iter()
        .map(|q| <&str>::from(q).to_owned())
        .collect()
}

/// Quantizations the page sent.
fn parsed(names: &[String]) -> Result<Vec<FlshQuantization>> {
    names
        .iter()
        .map(|q| {
            q.parse::<FlshQuantization>().map_err(|_| {
                DesktopError::BadRequest(format!("{q} is not a quantization OpenRouter knows."))
            })
        })
        .collect()
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
