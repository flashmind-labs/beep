use anyhow::{Context, Result};
use beep_server::{
    app::{App, ServerTools},
    apps::ComposioSettings,
    config::{self, Auth, Config},
    decision::DecisionTool,
    memories,
    search::SearchTool,
    store::Store,
    transport,
};
use flashmind::{
    llm::OpenRouterProvider,
    memory::{EmbeddingProvider, OpenAIEmbedding, OpenRouterEmbedding},
    types::{AgentLlmConfig, Model},
};
use iroh::endpoint::presets;
use iroh::protocol::Router;
use logforth::{
    append::{Stderr, file::FileBuilder},
    filter::rustlog::RustLogFilterBuilder,
};
use secrecy::SecretString;
use std::{num::NonZeroUsize, sync::Arc};

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".into());
    let config: Config = toml::from_str(
        &std::fs::read_to_string(&config_path).with_context(|| format!("reading {config_path}"))?,
    )?;
    start_logging(&config)?;

    let auth = Auth::from_config(&config)?;
    let model: Model = config.model.parse()?;
    let mut llm = AgentLlmConfig::new(model).with_reasoning(config.reasoning);
    // Reasoning shares this budget with the reply.
    llm.max_tokens = Some(8192);
    llm.provider_preferences = Some(config.routing.clone());
    let key = config
        .provider_key
        .as_deref()
        .context("provider_key is required for OpenRouter")?;
    let provider = Arc::new(OpenRouterProvider::new(key.to_owned()));

    config::ensure_parent_dir(&config.database)?;
    let store = Store::open(&config.database).await?;
    store.recover().await?;

    let search = config
        .firecrawl_key
        .as_ref()
        .map(|key| SearchTool::new(key.clone()).map(Arc::new))
        .transpose()?;
    let embedder: Arc<dyn EmbeddingProvider> = match &config.openai_key {
        Some(openai_key) => Arc::new(OpenAIEmbedding::new(
            Some(openai_key.clone()),
            memories::EMBEDDING_MODEL.to_owned(),
            None,
        )?),
        None => Arc::new(OpenRouterEmbedding::new(
            key.to_owned(),
            memories::OPENROUTER_EMBEDDING_MODEL.to_owned(),
        )?),
    };
    let decision = Some(Arc::new(DecisionTool::new(SecretString::from(key))?));
    let composio = config.composio_key.as_deref().map(|key| ComposioSettings {
        key: SecretString::from(key),
        callback_url: config.composio_callback_url.clone(),
    });
    let app = App::new(
        store,
        auth,
        provider,
        llm,
        ServerTools {
            search,
            firecrawl_key: config.firecrawl_key.as_deref().map(SecretString::from),
            decision,
            composio,
            embedder: Some(embedder),
        },
    );
    beep_server::mcp::start_keep_alive(app.clone());
    beep_server::compaction::start(app.clone());
    beep_server::titles::start(app.clone());
    beep_server::memories::start_curation(app.clone());
    beep_server::feeds::start(app.clone());
    let (cron_runner, _cron_registry) = beep_server::cron::start(app.clone(), &config.database);
    tokio::spawn(async move {
        if let Err(e) = cron_runner.start().await {
            log::error!("cron runner failed: {e}");
        }
    });

    let secret_key = config::load_secret_key(config.secret_key.as_deref())?;
    let endpoint = iroh::Endpoint::builder(presets::N0)
        .secret_key(secret_key)
        .bind()
        .await?;

    log::info!("iroh endpoint bound; node_id={}", endpoint.id());

    let handler = transport::BeepHandler::new(app.clone());
    let router = Router::builder(endpoint)
        .accept(transport::ALPN, handler)
        .spawn();

    log::info!("listening; addr={:?}", router.endpoint().addr());

    tokio::signal::ctrl_c().await?;
    router.shutdown().await?;
    app.shutdown().await;
    Ok(())
}

/// Daily log files kept before the oldest is deleted.
const LOG_FILES_KEPT: usize = 14;

/// Logs to stderr and to a file under `log_dir` that rolls over daily.
/// Debug by default; `RUST_LOG` overrides it.
fn start_logging(config: &Config) -> Result<()> {
    let dir = config.log_dir();
    let file = FileBuilder::new(&dir, "beep")
        .filename_suffix("log")
        .rollover_daily()
        .max_log_files(NonZeroUsize::new(LOG_FILES_KEPT).expect("the count is not zero"))
        .build()
        .with_context(|| format!("opening the log directory {}", dir.display()))?;
    let filter = || RustLogFilterBuilder::from_default_env_or("debug").build();
    logforth::starter_log::builder()
        .dispatch(|d| d.filter(filter()).append(Stderr::default()))
        .dispatch(|d| d.filter(filter()).append(file))
        .apply();
    log::info!("logging to {}", dir.display());
    Ok(())
}
