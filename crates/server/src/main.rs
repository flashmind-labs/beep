use anyhow::{Context, Result};
use beep_server::{
    app::App,
    config::{Auth, Config},
    store::Store,
    transport,
};
use flashmind::{llm::OpenRouterProvider, types::Model};
use iroh::SecretKey;
use iroh::endpoint::presets;
use iroh::protocol::Router;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    logforth::starter_log::stderr().apply();
    log::set_max_level(log::LevelFilter::Debug);

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

    if let Some(parent) = config
        .database
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let store = Store::open(&config.database).await?;
    store.recover().await?;
    let search = config
        .firecrawl_key
        .as_ref()
        .map(|key| beep_server::search::SearchTool::new(key.clone()).map(Arc::new))
        .transpose()?;
    let composio = config
        .composio_key
        .as_ref()
        .map(|key| beep_server::composio::ComposioClient::new(key.clone()).map(Arc::new))
        .transpose()?;
    let mcp = config
        .fastmail_token
        .as_deref()
        .map(|token| beep_server::mcp::fastmail_registry(token.to_owned()));
    let app = App::new(store, auth, provider, model, search, composio, mcp);
    let scheduler_app = app.clone();
    tokio::spawn(async move { beep_server::scheduler::run(scheduler_app).await });

    let secret_key = match &config.secret_key {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(data) => data.trim().parse::<SecretKey>()?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let key = SecretKey::generate();
                if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                    std::fs::create_dir_all(parent)?;
                }
                let hex: String = key.to_bytes().iter().map(|b| format!("{b:02x}")).collect();
                std::fs::write(path, &hex)?;
                log::info!("generated new secret key at {}", path.display());
                key
            }
            Err(err) => return Err(err).context("reading secret key"),
        },
        None => SecretKey::generate(),
    };

    let endpoint = iroh::Endpoint::builder(presets::N0)
        .secret_key(secret_key)
        .bind()
        .await?;

    let node_id = endpoint.id();
    log::info!("iroh endpoint bound; node_id={node_id}");

    let handler = transport::BeepHandler::new(app.clone());
    let router = Router::builder(endpoint)
        .accept(transport::ALPN, handler)
        .spawn();

    let addr = router.endpoint().addr();
    log::info!("listening; addr={addr:?}");

    tokio::signal::ctrl_c().await?;
    router.shutdown().await?;
    app.shutdown().await;
    Ok(())
}
