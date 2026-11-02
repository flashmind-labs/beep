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
use iroh::{EndpointId, SecretKey};
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
    /// Models Jev may pick per prompt for a user with Auto on, each with
    /// what it is good for. Only offered models take part; none leaves Auto
    /// unavailable.
    #[serde(default)]
    pub auto_models: HashMap<Model, String>,
    /// The removed per-prompt agents. Ignored; kept so configs that still
    /// have an `[agents]` table load.
    #[serde(default, rename = "agents")]
    _agents: Option<IgnoredAny>,
    /// The models research roles run on. Research is off without it.
    pub research: Option<ResearchConfig>,
    /// The model project chats plan on. Without it they plan on the chat's
    /// default.
    pub project: Option<ProjectConfig>,
    /// Endpoint ids of the users who get operator features: machines,
    /// account events and watchers.
    #[serde(default)]
    pub admins: Vec<EndpointId>,
    /// Describes images for models without vision, and for the saved
    /// conversation. Defaults to `model` when that one sees images.
    pub vision_model: Option<String>,
    /// Removed: sessions are summarized on the model their chat ran on.
    /// Ignored; kept so configs that still set it load.
    #[serde(default, rename = "compaction_model")]
    _compaction_model: Option<IgnoredAny>,
    /// How much the assistant thinks before answering: off, low, medium or high.
    #[serde(default = "default_reasoning")]
    pub reasoning: ReasoningLevel,
    /// OpenRouter provider routing. Replaces the default entirely when set.
    #[serde(default = "default_routing")]
    pub routing: ProviderPreferences,
    /// Endpoints OpenRouter tries first for a model, ahead of the sort in
    /// `routing`, such as `["openai/flex"]`. The rest of `routing` still
    /// applies, and other endpoints remain as fallbacks.
    #[serde(default)]
    pub provider_order: HashMap<Model, Vec<String>>,
    pub provider_key: Option<String>,
    pub firecrawl_key: Option<String>,
    /// OpenAI key for memory embeddings. Without it they go through OpenRouter.
    pub openai_key: Option<String>,
    pub composio_key: Option<String>,
    /// Mistral key for live transcription (`[voice] realtime_model`).
    /// Without it the apps' live voice says it is unavailable.
    pub mistral_key: Option<SecretString>,
    /// Where Composio sends the browser after an app connects, for example a
    /// deep link back into the phone app. Composio shows its own page if unset.
    pub composio_callback_url: Option<String>,
    /// Where the daily log files and the cron log go. Defaults to `logs`.
    pub log_dir: Option<PathBuf>,
    /// No longer used: code and files go to the user's environment, a node
    /// started with `--workspace`. Still accepted so older configs load.
    pub sandbox_dir: Option<PathBuf>,
    /// Firebase service account key file. With it, notices for phones the
    /// server cannot reach over iroh go through Firebase Cloud Messaging.
    pub fcm_credentials: Option<PathBuf>,
    /// Apple Push Notification service key. With it, notices for iPhones the
    /// server cannot reach over iroh go through APNs.
    pub apns: Option<ApnsConfig>,
    /// The models that speak notices as voice notes and transcribe spoken
    /// replies. Defaults to Grok's speech and Voxtral's transcription.
    #[serde(default)]
    pub voice: VoiceConfig,
    /// The model account events are triaged on (`triage_model`). Defaults
    /// to Claude Haiku thinking little.
    #[serde(default)]
    pub events: EventsConfig,
    /// Runs the server for one owner (see `crate::instance`). Without it
    /// every key that connects gets an account.
    pub instance: Option<InstanceConfig>,
    /// The agents that read finished chats: their model (`model`, a cheap
    /// one by default), how long a chat stays quiet first (`quiet_minutes`),
    /// how many run at once (`concurrency`), and per watcher (`[watchers.memory]`,
    /// `[watchers.skills]`, `[watchers.improvement]`) `enabled`, `model` and
    /// `timeout_secs`.
    #[serde(default)]
    pub watchers: WatchersConfig,
}

/// The config's `[project]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    /// What a project chat's planning turns (a new request, a re-plan) run
    /// on. Routine turns run on the chat's default. It must be offered.
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

/// Fastest provider that keeps no data (zero data retention) and serves at
/// least fp8.
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
    fn reads_admins_and_model_choices() {
        let admin = SecretKey::generate().public();
        let config: Config = toml::from_str(&format!(
            r#"
            model = "openrouter:z-ai/glm-5.3-flash"
            model_choices = ["openrouter:openai/gpt-6-luna"]
            admins = ["{admin}"]
            "#
        ))
        .expect("the config is valid");
        assert_eq!(config.admins, [admin]);
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
