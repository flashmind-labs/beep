use anyhow::{Context, Result};
use beep_server::{
    app::{App, ServerTools},
    apps::ComposioSettings,
    config::{self, Config},
    decision::DecisionTool,
    fcm::{Fallback, Firebase},
    memories,
    metering::Metered,
    models::ModelChoices,
    push::Pusher,
    sandbox::Sandbox,
    search::SearchTool,
    store::Store,
    transport,
};
use flashmind::{
    llm::OpenRouterProvider,
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

    let model: Model = config.model.parse()?;
    let mut llm = AgentLlmConfig::new(model).with_reasoning(config.reasoning);
    // Reasoning shares this budget with the reply.
    llm.max_tokens = Some(8192);
    llm.provider_preferences = Some(config.routing.clone());
    let key = config
        .provider_key
        .as_deref()
        .context("provider_key is required for OpenRouter")?;
    let embedder = memories::embedder(&config)?;
    let store = Store::open(&config.database_url()?, embedder).await?;
    store.recover().await?;
    let provider = Arc::new(
        Metered::new(
            Arc::new(OpenRouterProvider::new(key.to_owned())),
            store.clone(),
        )
        .await,
    );
    let models = ModelChoices::resolve(
        provider.as_ref(),
        llm.model.clone(),
        config.model_choices.clone(),
        &config.admins,
    )
    .await
    .with_routing(config.model_routing())
    .with_auto(config.auto_models.clone());

    let search = config
        .firecrawl_key
        .as_ref()
        .map(|key| SearchTool::new(key.clone()).map(Arc::new))
        .transpose()?;
    let decision = Some(Arc::new(DecisionTool::new(SecretString::from(key))?));
    let composio = config.composio_key.as_deref().map(|key| ComposioSettings {
        key: SecretString::from(key),
        callback_url: config.composio_callback_url.clone(),
    });
    let vision_model = config
        .vision_model
        .as_deref()
        .map(str::parse::<Model>)
        .transpose()?;
    let sandbox = match &config.sandbox_dir {
        Some(dir) => {
            let sandbox = Sandbox::new(dir.clone()).await?;
            sandbox.start_sweep();
            Some(sandbox)
        }
        None => None,
    };
    let app = App::new(
        store,
        provider,
        llm,
        ServerTools {
            search,
            firecrawl_key: config.firecrawl_key.as_deref().map(SecretString::from),
            decision,
            composio,
            vision_model,
            sandbox,
        },
        models,
    );
    // Subscribed before users start, so notices from their first crons wait
    // for the pusher instead of being dropped.
    let notices = app.notifications.subscribe();
    app.start_users().await?;
    beep_server::compaction::start(app.clone());
    beep_server::titles::start(app.clone());
    beep_server::memories::start_purge(app.clone());
    let (cron_runner, _cron_registry) = beep_server::cron::start(app.clone(), &config.log_dir());
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

    let fallback = match &config.fcm_credentials {
        Some(path) => Some(Arc::new(Firebase::new(app.clone(), path)?) as Arc<dyn Fallback>),
        None => {
            log::info!("fcm_credentials not set: notices do not go through Firebase");
            None
        }
    };
    let pusher = Pusher::new(endpoint.clone(), Arc::new(app.store.clone()), fallback);
    pusher.start(notices);
    let handler = transport::BeepHandler::new(app.clone(), pusher);
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
/// Debug by default, except html5ever, which logs every token of each HTML
/// email html2text reads. `RUST_LOG` overrides it.
fn start_logging(config: &Config) -> Result<()> {
    let dir = config.log_dir();
    let file = FileBuilder::new(&dir, "beep")
        .filename_suffix("log")
        .rollover_daily()
        .max_log_files(NonZeroUsize::new(LOG_FILES_KEPT).expect("the count is not zero"))
        .build()
        .with_context(|| format!("opening the log directory {}", dir.display()))?;
    let filter = || RustLogFilterBuilder::from_default_env_or("debug,html5ever=info").build();
    logforth::starter_log::builder()
        .dispatch(|d| d.filter(filter()).append(Stderr::default()))
        .dispatch(|d| d.filter(filter()).append(file))
        .apply();
    log::info!("logging to {}", dir.display());
    Ok(())
}
