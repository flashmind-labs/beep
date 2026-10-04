use std::{env, io, path::Path, path::PathBuf};

use anyhow::{Context, Result};
use flashmind::types::{ProviderPreferences, ProviderSort, Quantization, ReasoningLevel};
use iroh::SecretKey;
use secrecy::SecretString;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub secret_key: Option<PathBuf>,
    /// Postgres connection string. Falls back to `DATABASE_URL`.
    pub database_url: Option<SecretString>,
    pub model: String,
    /// Describes images for models without vision, and for the saved
    /// conversation. Defaults to `model` when that one sees images.
    pub vision_model: Option<String>,
    /// How much the assistant thinks before answering: off, low, medium or high.
    #[serde(default = "default_reasoning")]
    pub reasoning: ReasoningLevel,
    /// OpenRouter provider routing. Replaces the default entirely when set.
    #[serde(default = "default_routing")]
    pub routing: ProviderPreferences,
    pub provider_key: Option<String>,
    pub firecrawl_key: Option<String>,
    /// OpenAI key for memory embeddings. Without it they go through OpenRouter.
    pub openai_key: Option<String>,
    pub composio_key: Option<String>,
    /// Where Composio sends the browser after an app connects, for example a
    /// deep link back into the phone app. Composio shows its own page if unset.
    pub composio_callback_url: Option<String>,
    /// Where the daily log files and the cron log go. Defaults to `logs`.
    pub log_dir: Option<PathBuf>,
    /// Holds a folder per user for downloads and code the agent runs under
    /// Drop. The `execute_code` and `download_file` tools exist only when set.
    pub sandbox_dir: Option<PathBuf>,
}

impl Config {
    pub fn log_dir(&self) -> PathBuf {
        self.log_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("logs"))
    }

    /// The Postgres connection string from the config or `DATABASE_URL`.
    pub fn database_url(&self) -> Result<SecretString> {
        if let Some(url) = &self.database_url {
            return Ok(url.clone());
        }
        env::var("DATABASE_URL")
            .map(SecretString::from)
            .context("set database_url in the config or DATABASE_URL")
    }
}

fn default_reasoning() -> ReasoningLevel {
    ReasoningLevel::Low
}

/// Fastest provider that keeps no data and serves at least fp8.
fn default_routing() -> ProviderPreferences {
    ProviderPreferences {
        data_collection: Some("deny".into()),
        quantizations: vec![
            Quantization::Fp8,
            Quantization::Fp16,
            Quantization::Bf16,
            Quantization::Fp32,
            Quantization::Other("unknown".into()),
        ],
        sort: Some(ProviderSort::Throughput),
        ..Default::default()
    }
}

pub fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

pub fn load_secret_key(path: Option<&Path>) -> Result<SecretKey> {
    let Some(path) = path else {
        return Ok(SecretKey::generate());
    };
    match std::fs::read_to_string(path) {
        Ok(data) => Ok(data.trim().parse::<SecretKey>()?),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            let key = SecretKey::generate();
            ensure_parent_dir(path)?;
            let hex: String = key.to_bytes().iter().map(|b| format!("{b:02x}")).collect();
            std::fs::write(path, &hex)?;
            log::info!("generated new secret key at {}", path.display());
            Ok(key)
        }
        Err(err) => Err(err).context("reading secret key"),
    }
}
