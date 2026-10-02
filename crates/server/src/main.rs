use anyhow::{Context, Result};
use beep_server::{
    app::App,
    config::{Auth, Config},
    store::Store,
    transport,
};
use flashmind::{
    llm::{OpenAiProvider, OpenRouterProvider},
    types::{LlmProvider, Model, Provider},
};
use iroh::SecretKey;
use iroh::endpoint::presets;
use iroh::protocol::Router;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "beep_server=info".into()),
        )
        .init();
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".into());
    let config: Config = toml::from_str(
        &std::fs::read_to_string(&config_path).with_context(|| format!("reading {config_path}"))?,
    )?;
    let auth = Auth::from_config(&config)?;
    let model: Model = config.model.parse()?;
    let key = config.provider_key.as_deref();
    let provider: Arc<dyn LlmProvider> = match model.provider {
        Provider::OpenRouter => Arc::new(OpenRouterProvider::new(
            key.context("OpenRouter requires provider_key")?.to_owned(),
        )),
        Provider::OpenAi => {
            let url = config
                .provider_url
                .as_deref()
                .context("OpenAI provider requires provider_url")?;
            let mut builder = OpenAiProvider::builder(url);
            if let Some(key) = key {
                builder = builder.api_key(key);
            }
            Arc::new(builder.build()?)
        }
        _ => anyhow::bail!("This server supports openai: and openrouter: models"),
    };
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
    let app = App::with_tools(store, auth, provider, model, search, composio, mcp);
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
                tracing::info!("generated new secret key at {}", path.display());
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
    tracing::info!(%node_id, "iroh endpoint bound");

    let handler = transport::BeepHandler::new(app.clone());
    let router = Router::builder(endpoint)
        .accept(transport::ALPN, handler)
        .spawn();

    let addr = router.endpoint().addr();
    tracing::info!(?addr, "listening");

    tokio::signal::ctrl_c().await?;
    router.shutdown().await?;
    app.shutdown().await;
    Ok(())
}
