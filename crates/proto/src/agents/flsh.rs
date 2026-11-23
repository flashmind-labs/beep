//! flsh's own settings, which Beep sets for the user on every machine:
//! OpenRouter provider routing (zero data retention, allowed quantizations,
//! provider sort, fallbacks) and when flsh compacts a conversation, for all
//! models and per model. pi takes the routing too, in its `models.json`.
//!
//! Each field is one key of flsh's `config.toml`. Unset (`None`, or empty
//! for a list) means Beep leaves that key to flsh's own default.

use serde::{Deserialize, Serialize};
use std::{collections::HashSet, num::NonZeroU32};
use thiserror::Error;

/// A model as flsh names it, `provider:name` such as
/// `openrouter:moonshotai/kimi-k2`: flsh's own type, so a model Beep writes
/// is one flsh reads.
pub use flashmind_types::Model as FlshModel;
/// Who serves a model, the part of [`FlshModel`] before the colon.
pub use flashmind_types::Provider as FlshProvider;

/// The fewest tokens a compaction threshold may be: below it flsh would
/// compact on every turn.
const MIN_COMPACT_TOKENS: u32 = 1_000;
/// The most tokens a compaction threshold may be; no model's window is
/// larger.
const MAX_COMPACT_TOKENS: u32 = 10_000_000;
/// The most per-model compaction rules.
const MAX_MODEL_RULES: usize = 64;
/// The longest model name in a rule.
const MAX_MODEL_LEN: usize = 200;

wire_structs! {
    /// flsh's settings as the user set them in Beep.
    #[derive(Default)]
    #[serde(default)]
    pub struct FlshSettings {
        /// `openrouter.routing.zdr`: only endpoints that keep no data. `None`
        /// leaves it to the OpenRouter account's setting.
        pub zdr: Option<bool>,
        /// `openrouter.routing.quantizations`: the weight precisions allowed.
        /// Empty means any.
        pub quantizations: Vec<FlshQuantization>,
        /// `openrouter.routing.sort`. `None` is OpenRouter's default, which
        /// balances price and uptime.
        pub sort: Option<FlshSort>,
        /// `openrouter.routing.allow_fallbacks`: try other providers when the
        /// first ones fail. `None` is OpenRouter's default, which does.
        pub allow_fallbacks: Option<bool>,
        /// `[autocompact]`: when flsh compacts with no model rule. `None` is
        /// flsh's default, on at the model's own trigger.
        pub autocompact: Option<FlshAutoCompact>,
        /// Rules that replace the ones above for their model only.
        pub models: Vec<FlshModelRule>,
        /// Maximum silence after OpenRouter starts streaming. When unset,
        /// use the host's timeout or 300 seconds if it has none.
        #[serde(default)]
        pub stream_idle_timeout_secs: Option<NonZeroU32>,
    }

    /// When flsh compacts a conversation.
    #[derive(Copy, Eq)]
    pub struct FlshAutoCompact {
        /// Off: flsh waits for the context window to overflow.
        pub enabled: bool,
        /// Compact once the prompt passes this many tokens, if that comes
        /// before the model's own trigger. `None` is the model's trigger.
        pub tokens: Option<u32>,
    }

    /// The settings of one model.
    pub struct FlshModelRule {
        pub model: FlshModel,
        /// `[auto_compact_models."<model>"]`, unless `inherits_compaction`.
        /// A rule that inherits holds the rule for all models here, which
        /// is what a node that predates `inherits_compaction` writes.
        pub autocompact: FlshAutoCompact,
        /// The model compacts as all models do.
        #[serde(default)]
        pub inherits_compaction: bool,
        /// `[openrouter.model_quantizations]`: the weight precisions allowed
        /// for this OpenRouter model, empty for any. `None` keeps the list
        /// for all models.
        #[serde(default)]
        pub quantizations: Option<Vec<FlshQuantization>>,
    }
}

/// How OpenRouter ranks the providers of a model. Its text (`price`,
/// `throughput`, `latency`) is what flsh's config takes.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    strum::IntoStaticStr,
    strum::EnumString,
    strum::VariantArray,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum FlshSort {
    Price,
    Throughput,
    Latency,
}

/// A weight precision OpenRouter can filter providers by. Its text, such as
/// `fp8`, is what flsh's config takes.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    strum::IntoStaticStr,
    strum::EnumString,
    strum::VariantArray,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum FlshQuantization {
    Int4,
    Int8,
    Fp4,
    Fp6,
    Fp8,
    Fp16,
    Bf16,
    Fp32,
    /// Providers that do not say.
    Unknown,
}

/// Why settings cannot be saved.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FlshSettingsError {
    #[error("{0} is listed twice in the allowed quantizations.")]
    DuplicateQuantization(&'static str),
    #[error(
        "A compaction threshold must be between {MIN_COMPACT_TOKENS} and {MAX_COMPACT_TOKENS} tokens."
    )]
    Tokens,
    #[error("At most {MAX_MODEL_RULES} models can have their own compaction rule.")]
    TooManyModels,
    #[error("{0} has two compaction rules.")]
    DuplicateModel(String),
    #[error(
        "A model is named provider:model, such as openrouter:vendor/model, in at most {MAX_MODEL_LEN} characters."
    )]
    BadModel,
    #[error("{0} is not an OpenRouter model, so it has no quantizations to allow.")]
    NotOpenRouter(String),
}

impl FlshAutoCompact {
    fn check(&self) -> Result<(), FlshSettingsError> {
        match self.tokens {
            Some(tokens) if !(MIN_COMPACT_TOKENS..=MAX_COMPACT_TOKENS).contains(&tokens) => {
                Err(FlshSettingsError::Tokens)
            }
            _ => Ok(()),
        }
    }
}

impl FlshSettings {
    /// Whether these settings can be written to flsh's config as they are.
    pub fn check(&self) -> Result<(), FlshSettingsError> {
        unique(&self.quantizations)?;

        if let Some(autocompact) = &self.autocompact {
            autocompact.check()?;
        }

        if self.models.len() > MAX_MODEL_RULES {
            return Err(FlshSettingsError::TooManyModels);
        }

        let mut models = HashSet::new();

        for rule in &self.models {
            let name = rule.model.to_string();
            let plain = name.len() <= MAX_MODEL_LEN
                && !name.chars().any(|c| c.is_control())
                && !rule.model.name().is_empty();
            if !plain {
                return Err(FlshSettingsError::BadModel);
            }

            if !models.insert(name.clone()) {
                return Err(FlshSettingsError::DuplicateModel(name));
            }

            rule.autocompact.check()?;

            if let Some(quantizations) = &rule.quantizations {
                if rule.model.provider != FlshProvider::OpenRouter {
                    return Err(FlshSettingsError::NotOpenRouter(name));
                }

                unique(quantizations)?;
            }
        }

        Ok(())
    }
}

/// An error when a quantization is listed twice.
fn unique(quantizations: &[FlshQuantization]) -> Result<(), FlshSettingsError> {
    let mut seen = HashSet::new();

    for quantization in quantizations {
        if !seen.insert(*quantization) {
            return Err(FlshSettingsError::DuplicateQuantization(
                quantization.into(),
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "../tests/agents_flsh.rs"]
mod tests;
