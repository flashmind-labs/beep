//! flsh's own settings, which Beep sets for the user on every machine:
//! OpenRouter provider routing (zero data retention, allowed quantizations,
//! provider sort, fallbacks) and when flsh compacts a conversation, for all
//! models and per model.
//!
//! Each field is one key of flsh's `config.toml`. Unset (`None`, or empty
//! for a list) means Beep leaves that key to flsh's own default.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

/// A model as flsh names it, `provider:name` such as
/// `openrouter:moonshotai/kimi-k2`: flsh's own type, so a model Beep writes
/// is one flsh reads.
pub use flashmind_types::Model as FlshModel;

/// The fewest tokens a compaction threshold may be: below it flsh would
/// compact on every turn.
pub const MIN_COMPACT_TOKENS: u32 = 1_000;
/// The most tokens a compaction threshold may be; no model's window is
/// larger.
pub const MAX_COMPACT_TOKENS: u32 = 10_000_000;
/// The most per-model compaction rules.
pub const MAX_MODEL_RULES: usize = 64;
/// The longest model name in a rule.
pub const MAX_MODEL_LEN: usize = 200;

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
        /// `[auto_compact_models."<model>"]`: rules that replace the one above
        /// for their model only.
        pub models: Vec<FlshModelRule>,
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

    /// The compaction rule for one model.
    pub struct FlshModelRule {
        pub model: FlshModel,
        pub autocompact: FlshAutoCompact,
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
        let mut seen = HashSet::new();
        for quantization in &self.quantizations {
            if !seen.insert(*quantization) {
                return Err(FlshSettingsError::DuplicateQuantization(
                    quantization.into(),
                ));
            }
        }
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
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(name: &str) -> FlshModel {
        name.parse().expect("a valid model")
    }

    fn rule(name: &str, tokens: Option<u32>) -> FlshModelRule {
        FlshModelRule {
            model: model(name),
            autocompact: FlshAutoCompact {
                enabled: true,
                tokens,
            },
        }
    }

    #[test]
    fn bad_values_are_refused() {
        let twice = FlshSettings {
            quantizations: vec![FlshQuantization::Fp8, FlshQuantization::Fp8],
            ..FlshSettings::default()
        };
        assert_eq!(
            twice.check(),
            Err(FlshSettingsError::DuplicateQuantization("fp8"))
        );
        for tokens in [0, 999, MAX_COMPACT_TOKENS + 1] {
            let global = FlshSettings {
                autocompact: Some(FlshAutoCompact {
                    enabled: true,
                    tokens: Some(tokens),
                }),
                ..FlshSettings::default()
            };
            assert_eq!(global.check(), Err(FlshSettingsError::Tokens));
            let per_model = FlshSettings {
                models: vec![rule("openrouter:vendor/model", Some(tokens))],
                ..FlshSettings::default()
            };
            assert_eq!(per_model.check(), Err(FlshSettingsError::Tokens));
        }
        // The same model spelled two ways is still one model.
        let duplicate = FlshSettings {
            models: vec![
                rule("openrouter:vendor/model", None),
                rule("OpenRouter:vendor/model", Some(50_000)),
            ],
            ..FlshSettings::default()
        };
        assert!(matches!(
            duplicate.check(),
            Err(FlshSettingsError::DuplicateModel(_))
        ));
        let long = FlshSettings {
            models: vec![rule(&format!("openrouter:{}", "x".repeat(300)), None)],
            ..FlshSettings::default()
        };
        assert_eq!(long.check(), Err(FlshSettingsError::BadModel));
        let many = FlshSettings {
            models: (0..=MAX_MODEL_RULES)
                .map(|i| rule(&format!("openrouter:vendor/m{i}"), None))
                .collect(),
            ..FlshSettings::default()
        };
        assert_eq!(many.check(), Err(FlshSettingsError::TooManyModels));
    }

    #[test]
    fn sorts_and_quantizations_spell_as_flsh_does() {
        use strum::VariantArray;
        for q in FlshQuantization::VARIANTS {
            let text: &str = q.into();
            let wire = rmp_serde::to_vec(q).expect("encodes");
            assert_eq!(wire, rmp_serde::to_vec(text).expect("encodes"));
            assert_eq!(text.parse::<FlshQuantization>().ok(), Some(*q));
        }
        assert!("fp7".parse::<FlshQuantization>().is_err());
        // flsh's own enum reads what Beep writes.
        for sort in FlshSort::VARIANTS {
            let text: &str = sort.into();
            let wire = rmp_serde::to_vec(sort).expect("encodes");
            assert_eq!(wire, rmp_serde::to_vec(text).expect("encodes"));
            let theirs: flashmind_types::ProviderSort =
                rmp_serde::from_slice(&wire).expect("flsh reads it");
            assert_eq!(rmp_serde::to_vec(&theirs).expect("encodes"), wire);
        }
    }
}
