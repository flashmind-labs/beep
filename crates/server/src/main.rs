use anyhow::{Context, Result};
use beep_server::{
    app::App,
    config::{self, Auth, Config},
    store::Store,
    transport,
};
use flashmind::{llm::OpenRouterProvider, types::Model};
use iroh::endpoint::presets;
use iroh::protocol::Router;
use logforth::record::{Level, LevelFilter};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    logforth::starter_log::stderr()
        .filter(LevelFilter::MoreSevereEqual(Level::Debug))
        .apply();

    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".into());
    let config: Config = toml::from_str(
        &std::fs::read_to_string(&config_path).with_context(|| format!("reading {config_path}"))?,
    )?;

    let auth = Auth::from_config(&config)?;
    let model: Model = config.model.parse()?;
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
        .map(|key| beep_server::search::SearchTool::new(key.clone()).map(Arc::new))
        .transpose()?;
    let composio_key = config.composio_key.clone();
    let mcp = config
        .fastmail_token
        .as_deref()
        .map(|token| beep_server::mcp::fastmail_registry(token.to_owned()));

    let app = App::new(store, auth, provider, model, search, composio_key, mcp);
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
