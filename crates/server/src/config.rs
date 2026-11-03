use std::{
    collections::HashMap,
    env,
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
    path::PathBuf,
};

use crate::{
    events::EventsConfig,
    instance::InstanceConfig,
    research::{ResearchConfig, RoleModel},
    voice::VoiceConfig,
    watchers::WatchersConfig,
};
use anyhow::{Context, Result};
use flashmind::types::{Model, ProviderPreferences, ProviderSort, Quantization, ReasoningLevel};
use iroh::SecretKey;
use secrecy::SecretString;
use serde::{Deserialize, de::IgnoredAny};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub secret_key: Option<PathBuf>,
    /// Postgres connection string. Falls back to `DATABASE_URL`.
    pub database_url: Option<SecretString>,
    pub model: String,
    /// Models users can pick for their own turns, besides `model`.
    #[serde(default)]
    pub model_choices: Vec<Model>,
    /// Models Jev may pick per prompt with Auto on; none leaves Auto unavailable.
    #[serde(default)]
    pub auto_models: HashMap<Model, String>,
    /// Removed per-prompt agents, kept so old configs load.
    #[serde(default, rename = "agents")]
    _agents: Option<IgnoredAny>,
    /// The models research roles run on. Research is off without it.
    pub research: Option<ResearchConfig>,
    /// The model project chats plan on; defaults to the chat's.
    pub project: Option<ProjectConfig>,
    /// Removed admin list, kept so old configs load.
    #[serde(default, rename = "admins")]
    _admins: Option<IgnoredAny>,
    /// Describes images for models without vision; defaults to `model` when it sees images.
    pub vision_model: Option<String>,
    /// Removed, kept so old configs load.
    #[serde(default, rename = "compaction_model")]
    _compaction_model: Option<IgnoredAny>,
    /// How much the assistant thinks before answering: off, low, medium or high.
    #[serde(default = "default_reasoning")]
    pub reasoning: ReasoningLevel,
    /// OpenRouter provider routing. Replaces the default entirely when set.
    #[serde(default = "default_routing")]
    pub routing: ProviderPreferences,
    /// Endpoints OpenRouter tries first for a model, such as `["openai/flex"]`.
    #[serde(default)]
    pub provider_order: HashMap<Model, Vec<String>>,
    pub provider_key: Option<String>,
    pub firecrawl_key: Option<String>,
    /// OpenAI key for memory embeddings. Without it they go through OpenRouter.
    pub openai_key: Option<String>,
    pub composio_key: Option<String>,
    /// Mistral key for live transcription.
    pub mistral_key: Option<SecretString>,
    /// Where Composio sends the browser after an app connects.
    pub composio_callback_url: Option<String>,
    /// Where the daily log files and the cron log go. Defaults to `logs`.
    pub log_dir: Option<PathBuf>,
    /// Unused, kept so old configs load.
    pub sandbox_dir: Option<PathBuf>,
    /// Firebase service account key file, for phones iroh cannot reach.
    pub fcm_credentials: Option<PathBuf>,
    /// APNs key, for iPhones iroh cannot reach.
    pub apns: Option<ApnsConfig>,
    /// Speech and transcription models for voice notes.
    #[serde(default)]
    pub voice: VoiceConfig,
    /// The model account events are triaged on.
    #[serde(default)]
    pub events: EventsConfig,
    /// Runs the server for one owner (see `crate::instance`).
    pub instance: Option<InstanceConfig>,
    /// The agents that read finished chats.
    #[serde(default)]
    pub watchers: WatchersConfig,
}

/// The config's `[project]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    /// The model for a project chat's planning turns; it must be offered.
    pub planner: RoleModel,
}

/// A token-based APNs key from the Apple Developer account.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApnsConfig {
    /// The `.p8` key file.
    pub key_file: PathBuf,
    pub key_id: String,
    pub team_id: String,
    /// The app's bundle id.
    pub topic: String,
}

impl Config {
    /// `routing` with the configured order, for each model that has one.
    pub fn model_routing(&self) -> HashMap<Model, ProviderPreferences> {
        self.provider_order
            .iter()
            .map(|(model, order)| {
                let routing = ProviderPreferences {
                    order: order.clone(),
                    ..self.routing.clone()
                };
                (model.clone(), routing)
            })
            .collect()
    }

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

/// Fastest zero-data-retention provider serving at least fp8.
fn default_routing() -> ProviderPreferences {
    ProviderPreferences {
        data_collection: Some("deny".into()),
        zdr: Some(true),
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
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

pub fn load_secret_key(path: Option<&Path>) -> Result<SecretKey> {
    let Some(path) = path else {
        return Ok(SecretKey::generate());
    };
    match fs::read_to_string(path) {
        Ok(data) => Ok(data.trim().parse::<SecretKey>()?),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            let key = SecretKey::generate();
            ensure_parent_dir(path)?;
            let hex: String = key.to_bytes().iter().map(|b| format!("{b:02x}")).collect();
            // Only the server's user may read the key.
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)
                .with_context(|| format!("writing {}", path.display()))?
                .write_all(hex.as_bytes())?;
            log::info!("generated new secret key at {}", path.display());
            Ok(key)
        }
        Err(err) => Err(err).context("reading secret key"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn writes_a_new_secret_key_for_the_owner_only() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = dir.path().join("keys/server.key");
        let key = load_secret_key(Some(&path)).expect("a new key");
        let mode = fs::metadata(&path)
            .expect("the key file")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let again = load_secret_key(Some(&path)).expect("the same key");
        assert_eq!(again.public(), key.public());
    }

    #[test]
    fn a_config_with_agent_tables_still_loads() {
        let config: Config = toml::from_str(
            r#"
            model = "openrouter:z-ai/glm-5.3-flash"
            model_choices = ["openrouter:openai/gpt-6-luna"]

            [agents.general]
            name = "General"
            when = "Anything"
            model = "openrouter:z-ai/glm-5.3-flash"
            thinking = "low"

            [agents.shopping]
            name = "Shopping"
            when = "Buying things"
            model = "openrouter:openai/gpt-6-luna"
            thinking = "medium"
            "#,
        )
        .expect("old agent tables are ignored");
        assert_eq!(config.model_choices.len(), 1);
    }

    #[test]
    fn ignores_admins_and_reads_model_choices() {
        let admin = SecretKey::generate().public();
        let config: Config = toml::from_str(&format!(
            r#"
            model = "openrouter:z-ai/glm-5.3-flash"
            model_choices = ["openrouter:openai/gpt-6-luna"]
            admins = ["{admin}"]
            "#
        ))
        .expect("old admin lists are ignored");
        assert_eq!(config.model_choices[0].name(), "openai/gpt-6-luna");
    }

    #[test]
    fn reads_an_instance_table() {
        let owner = SecretKey::generate().public();
        let config: Config = toml::from_str(&format!(
            r#"
            model = "openrouter:z-ai/glm-5.3-flash"
            [instance]
            owner = "{owner}"
            events = false
            local_node = "/link"
            "#
        ))
        .expect("the config is valid");
        let instance = config.instance.expect("an instance table");
        assert_eq!(instance.owner, Some(owner));
        assert!(!instance.events && instance.nodes);
        let config: Config = toml::from_str(r#"model = "openrouter:z-ai/glm-5.3-flash""#)
            .expect("the config is valid");
        assert!(config.instance.is_none());
    }

    #[test]
    fn reads_auto_models() {
        let config: Config = toml::from_str(
            r#"
            model = "openrouter:z-ai/glm-5.3-flash"
            [auto_models]
            "openrouter:openai/gpt-6-luna" = "Demanding work"
            "openrouter:z-ai/glm-5.3-flash" = "Quick lookups"
            "#,
        )
        .expect("the config is valid");
        let luna: Model = "openrouter:openai/gpt-6-luna"
            .parse()
            .expect("a valid model");
        assert_eq!(config.auto_models[&luna], "Demanding work");
        assert_eq!(config.auto_models.len(), 2);
    }

    #[test]
    fn provider_order_keeps_the_rest_of_the_routing() {
        let config: Config = toml::from_str(
            r#"
            model = "openrouter:z-ai/glm-5.3-flash"
            [provider_order]
            "openrouter:openai/gpt-6-luna" = ["openai/flex"]
            "#,
        )
        .expect("the config is valid");
        let luna: Model = "openrouter:openai/gpt-6-luna"
            .parse()
            .expect("a valid model");
        let routing = &config.model_routing()[&luna];
        assert_eq!(routing.order, ["openai/flex"]);
        assert_eq!(routing.data_collection.as_deref(), Some("deny"));
        assert_eq!(routing.zdr, Some(true));
        assert!(matches!(routing.sort, Some(ProviderSort::Throughput)));
    }
}
