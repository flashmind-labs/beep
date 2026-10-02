use anyhow::{Context, Result};
use flashmind::{
    llm::{OpenAiProvider, OpenRouterProvider},
    types::{LlmProvider, Model, Provider},
};
use neobeep_server::{
    app::App,
    config::{Auth, Config},
    store::Store,
    transport, web,
};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "neobeep_server=info".into()),
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
    let key = config
        .provider_key_env
        .as_ref()
        .map(|name| {
            let key = std::env::var(name)
                .with_context(|| format!("missing provider key environment variable {name}"))?;
            anyhow::ensure!(!key.trim().is_empty(), "provider key must not be empty");
            Ok::<_, anyhow::Error>(key)
        })
        .transpose()?;
    let provider: Arc<dyn LlmProvider> = match model.provider {
        Provider::OpenRouter => Arc::new(OpenRouterProvider::new(
            key.context("OpenRouter requires provider_key_env")?,
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
        .firecrawl_key_env
        .as_ref()
        .map(|name| {
            let key = std::env::var(name)
                .with_context(|| format!("missing search key environment variable {name}"))?;
            neobeep_server::search::SearchTool::new(key).map(Arc::new)
        })
        .transpose()?;
    let composio = config
        .composio_key_env
        .as_ref()
        .map(|name| {
            let key = std::env::var(name)
                .with_context(|| format!("missing Composio key environment variable {name}"))?;
            neobeep_server::composio::ComposioClient::new(key).map(Arc::new)
        })
        .transpose()?;
    let mcp = config
        .fastmail_token_env
        .as_ref()
        .map(|name| {
            let token = std::env::var(name)
                .with_context(|| format!("missing Fastmail token environment variable {name}"))?;
            anyhow::ensure!(!token.trim().is_empty(), "Fastmail token must not be empty");
            Ok::<_, anyhow::Error>(neobeep_server::mcp::fastmail_registry(token))
        })
        .transpose()?;
    let app = App::with_tools(store, auth, provider, model, search, composio, mcp);
    let scheduler_app = app.clone();
    tokio::spawn(async move { neobeep_server::scheduler::run(scheduler_app).await });
    let mut endpoints = Vec::new();
    let mut listeners = tokio::task::JoinSet::new();
    for address in std::iter::once(config.listen).chain(config.additional_listen) {
        if config.web_enabled {
            let listener = tokio::net::TcpListener::bind(address).await?;
            listeners.spawn(web::serve(listener, app.clone()));
            tracing::info!(%address, "Beep web UI listening on HTTP (TCP)");
        }
        let endpoint = transport::endpoint(
            address,
            transport::certificates(&config.certificate)?,
            transport::private_key(&config.private_key)?,
        )?;
        tracing::info!(address=%endpoint.local_addr()?,"Neobeep listening on HTTP/3 over QUIC (UDP)");
        listeners.spawn(transport::serve(endpoint.clone(), app.clone()));
        endpoints.push(endpoint);
    }
    tokio::select! {
        result = listeners.join_next() => { if let Some(result) = result { result??; } },
        result = tokio::signal::ctrl_c() => { result?; }
    }
    for endpoint in endpoints {
        endpoint.close(0u32.into(), b"server shutdown");
    }
    app.shutdown().await;
    Ok(())
}
