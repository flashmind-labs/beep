//! The config's `[watchers]`.

use super::WatcherKind;
use flashmind::types::{AliasedModel, Model, Provider};
use serde::Deserialize;
use std::time::Duration;

/// The config's `[watchers]` section.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WatchersConfig {
    /// The model every watcher runs on unless its own section names one.
    #[serde(default = "default_model")]
    pub model: Model,
    /// Minutes a chat with new messages stays quiet before it is read.
    #[serde(default = "default_quiet_minutes")]
    pub quiet_minutes: u64,
    /// Watchers running at once across the server.
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
    #[serde(default)]
    pub memory: WatcherConfig,
    #[serde(default)]
    pub skills: WatcherConfig,
    #[serde(default)]
    pub improvement: WatcherConfig,
}

/// One watcher's section, such as `[watchers.memory]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WatcherConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Overrides `[watchers] model`.
    pub model: Option<Model>,
    /// How long one review may take before it counts as failed.
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            model: None,
            timeout_secs: default_timeout_secs(),
        }
    }
}

impl Default for WatchersConfig {
    fn default() -> Self {
        Self {
            model: default_model(),
            quiet_minutes: default_quiet_minutes(),
            concurrency: default_concurrency(),
            memory: WatcherConfig::default(),
            skills: WatcherConfig::default(),
            improvement: WatcherConfig::default(),
        }
    }
}

impl WatchersConfig {
    pub fn watcher(&self, kind: WatcherKind) -> &WatcherConfig {
        match kind {
            WatcherKind::Memory => &self.memory,
            WatcherKind::Skills => &self.skills,
            WatcherKind::Improvement => &self.improvement,
        }
    }

    pub fn model(&self, kind: WatcherKind) -> &Model {
        self.watcher(kind).model.as_ref().unwrap_or(&self.model)
    }

    pub fn quiet(&self) -> Duration {
        Duration::from_secs(self.quiet_minutes.saturating_mul(60))
    }
}

fn default_model() -> Model {
    Model {
        provider: Provider::OpenRouter,
        model: AliasedModel {
            name: "anthropic/claude-haiku-5.5".to_owned(),
            real_name: None,
        },
    }
}

fn default_quiet_minutes() -> u64 {
    30
}

fn default_concurrency() -> usize {
    2
}

fn default_enabled() -> bool {
    true
}

fn default_timeout_secs() -> u64 {
    120
}
