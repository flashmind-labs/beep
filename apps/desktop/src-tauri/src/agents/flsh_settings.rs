//! flsh's own settings on the Coding agents page: OpenRouter routing and
//! auto-compact, the same on every machine. The page sends plain text; it
//! is parsed into the protocol's types here, and checked before it goes.

use crate::{
    beep::Service,
    error::{DesktopError, Result},
    projects::unknown_op,
    settings::credentials::plain,
};
use beep_proto::{
    FlshAutoCompact, FlshModel, FlshModelRule, FlshQuantization, FlshSettings, FlshSort,
    GetFlshSettings, SaveFlshSettings,
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
    autocompact: Option<AutoCompactView>,
    models: Vec<ModelRuleView>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct AutoCompactView {
    enabled: bool,
    tokens: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ModelRuleView {
    /// `provider:model`, such as `openrouter:vendor/model`.
    model: String,
    enabled: bool,
    tokens: Option<u32>,
}

impl From<FlshAutoCompact> for AutoCompactView {
    fn from(rule: FlshAutoCompact) -> Self {
        Self {
            enabled: rule.enabled,
            tokens: rule.tokens,
        }
    }
}

impl From<AutoCompactView> for FlshAutoCompact {
    fn from(rule: AutoCompactView) -> Self {
        Self {
            enabled: rule.enabled,
            tokens: rule.tokens,
        }
    }
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
            autocompact: settings.autocompact.map(Into::into),
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
            autocompact: view.autocompact.map(Into::into),
            models,
        };
        settings.check().map_err(|e| bad(e.to_string()))?;
        Ok(settings)
    }
}

/// The user's flsh settings; `None` from a server too old to keep them.
#[tauri::command]
pub async fn flsh_settings_get(beep: Service<'_>) -> Result<Option<FlshView>> {
    match beep.call(GetFlshSettings {}).await {
        Ok(settings) => Ok(Some(settings.into())),
        Err(e) if unknown_op(&e) => Ok(None),
        Err(e) => Err(plain(e)),
    }
}

/// Saves the user's flsh settings for every machine.
#[tauri::command]
pub async fn flsh_settings_save(beep: Service<'_>, settings: FlshView) -> Result<()> {
    let settings = FlshSettings::try_from(settings)?;
    beep.call(SaveFlshSettings { settings })
        .await
        .map_err(plain)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> FlshView {
        FlshView {
            zdr: Some(true),
            quantizations: vec!["fp8".into(), "bf16".into()],
            sort: Some("throughput".into()),
            allow_fallbacks: None,
            autocompact: Some(AutoCompactView {
                enabled: true,
                tokens: Some(300_000),
            }),
            models: vec![ModelRuleView {
                model: "openrouter:vendor/model".into(),
                enabled: false,
                tokens: None,
            }],
        }
    }

    #[test]
    fn the_page_round_trips() {
        let settings = FlshSettings::try_from(view()).expect("valid settings");
        assert_eq!(settings.sort, Some(FlshSort::Throughput));
        assert_eq!(FlshView::from(settings), view());
    }

    #[test]
    fn bad_text_is_refused() {
        let quant = FlshView {
            quantizations: vec!["fp7".into()],
            ..view()
        };
        assert!(FlshSettings::try_from(quant).is_err());
        let sort = FlshView {
            sort: Some("fastest".into()),
            ..view()
        };
        assert!(FlshSettings::try_from(sort).is_err());
        let model = FlshView {
            models: vec![ModelRuleView {
                model: "vendor/model".into(),
                enabled: true,
                tokens: None,
            }],
            ..view()
        };
        assert!(FlshSettings::try_from(model).is_err());
        let tokens = FlshView {
            autocompact: Some(AutoCompactView {
                enabled: true,
                tokens: Some(10),
            }),
            ..view()
        };
        assert!(FlshSettings::try_from(tokens).is_err());
    }
}
