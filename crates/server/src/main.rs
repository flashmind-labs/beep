use anyhow::{Context, Result};
use beep_proto::logging::{self, RollingFile};
use beep_server::{
    account::credentials::CredentialKey,
    account::instance,
    agent::cache_session::SharedCacheSessions,
    agent::decision::DecisionTool,
    agent::metering::Metered,
    agent::models::ModelChoices,
    api::transport,
    app::{App, ServerTools, admin},
    apps::ComposioSettings,
    config::{self, Config},
    memory::memories,
    nodes,
    push::Pusher,
    push::apns::Apns,
    push::fcm::Firebase,
    push::remote::{Fallback, RemotePush},
    store::Store,
    tools::Transitous,
    tools::search::SearchTool,
    voice::stt::{MistralStt, RealtimeStt},
};
use flashmind::{
    llm::{MistralProvider, OpenRouterProvider},
    types::{AgentLlmConfig, LlmProvider, Model},
};
use iroh::{Endpoint, endpoint::presets, protocol::Router};
use secrecy::SecretString;
use std::{env, fs, path::Path, sync::Arc, time::SystemTime};

/// `beep-server [config]` serves; `beep-server link [config]` prints the link code.
#[tokio::main]
async fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let first = args.next();
    if first.as_deref() == Some("link") {
        let config_path = args.next().unwrap_or_else(|| "config.toml".into());
        return print_link(&read_config(&config_path)?);
    }
    let config_path = first.unwrap_or_else(|| "config.toml".into());
    let config = read_config(&config_path)?;
    start_logging(&config)?;

    let model: Model = config.model.parse()?;
    let mut llm = AgentLlmConfig::new(model).with_reasoning(config.reasoning);
    // Reasoning shares this budget with the reply, and at high effort it can
    // use most of 8k before a long table is written.
    llm.max_tokens = Some(32_000);
    llm.provider_preferences = Some(config.routing.clone());
    let key = config
        .provider_key
        .as_deref()
        .context("provider_key is required for OpenRouter")?;
    let embedder = memories::embedder(&config)?;
    let store = Store::open(&config.database_url()?, embedder).await?;
    store.recover().await?;
    // Loaded before the provider, which names its cache sessions after it.
    let secret_key = config::load_secret_key(config.secret_key.as_deref())?;
    let provider = Arc::new(
        Metered::new(
            Arc::new(SharedCacheSessions::new(
                Arc::new(OpenRouterProvider::new(key.to_owned())),
                &secret_key.public(),
            )),
            store.clone(),
        )
        .await,
    );
    let models = ModelChoices::resolve(
        provider.as_ref(),
        llm.model.clone(),
        config.model_choices.clone(),
    )
    .await
    .with_routing(config.model_routing())
    .with_auto(config.auto_models.clone())
    .with_research(config.research.clone())
    .with_project(config.project.clone());

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
    if config.sandbox_dir.is_some() {
        log::warn!(
            "sandbox_dir is no longer used: code and files go to the user's environment, a \
             node started with --workspace; remove it from the config"
        );
    }
    let mistral = config
        .mistral_key
        .clone()
        .map(|key| Arc::new(MistralProvider::new(key)));
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
            transitous: Some(Arc::new(Transitous::new()?)),
            voice: config.voice.clone(),
            realtime: realtime(mistral.as_ref(), &config),
            mistral: mistral.map(|m| m as Arc<dyn LlmProvider>),
            instance: config.instance.clone(),
            credential_key: Some(CredentialKey::derive(&secret_key)),
        },
        models,
    );
    // Subscribed before users start, so notices from their first crons wait
    // for the pusher instead of being dropped.
    let notices = app.notifications.subscribe();
    app.start_users().await?;
    admin::serve(app.clone(), admin::default_path(Path::new(&config_path)))?;
    beep_server::agent::compaction::start(app.clone());
    beep_server::agent::titles::start(app.clone());
    beep_server::projects::start(app.clone());
    beep_server::projects::repo_watch::start(&app);
    beep_server::memory::memories::start_purge(app.clone());
    beep_server::store::start_pruning(app.store.clone());
    beep_server::routines::events::start(
        &app,
        beep_server::routines::events::Settings {
            triage: config.events.triage_model.clone(),
            ..beep_server::routines::events::Settings::default()
        },
    );
    beep_server::memory::watchers::start(
        &app,
        beep_server::memory::watchers::Settings::new(&config.watchers, app.llm()),
    );
    let cron_runner = beep_server::routines::cron::start(app.clone(), &config.log_dir());
    tokio::spawn(async move {
        if let Err(e) = cron_runner.start().await {
            log::error!("cron runner failed: {e}");
        }
    });

    let builder = || Endpoint::builder(presets::N0).secret_key(secret_key.clone());
    let port = config.port.unwrap_or(beep_proto::port::SERVER_PORT);
    let endpoint = beep_proto::port::bind_on(builder, port).await?;

    log::info!("iroh endpoint bound; node_id={}", endpoint.id());
    if let Some(instance) = app.instance() {
        instance.announce(endpoint.id())?;
        // First run: nobody owns it yet, so show how to connect.
        if app.instance_owner().await?.is_none() {
            println!(
                "{}",
                instance::link_code(&secret_key, instance.name(), SystemTime::now())?
            );
        }
    }

    let firebase = match &config.fcm_credentials {
        Some(path) => Some(Firebase::new(path)?),
        None => {
            log::info!("fcm_credentials not set: notices do not go through Firebase");
            None
        }
    };
    let apns = match &config.apns {
        Some(apns) => Some(Apns::new(apns)?),
        None => {
            log::info!("apns not set: notices do not go through APNs");
            None
        }
    };
    let fallback = (firebase.is_some() || apns.is_some())
        .then(|| Arc::new(RemotePush::new(app.store.clone(), firebase, apns)) as Arc<dyn Fallback>);
    let pusher = Pusher::new(endpoint.clone(), Arc::new(app.store.clone()), fallback);
    pusher.start(notices);
    let handler = transport::BeepHandler::new(app.clone(), pusher);
    let node_handler = nodes::NodeHandler::new(app.clone(), endpoint.id());
    let router = Router::builder(endpoint)
        .accept(transport::ALPN, handler)
        .accept(nodes::NODE_ALPN, node_handler)
        .spawn();

    log::info!("listening; addr={:?}", router.endpoint().addr());

    tokio::signal::ctrl_c().await?;
    router.shutdown().await?;
    app.shutdown().await;
    Ok(())
}

fn read_config(path: &str) -> Result<Config> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
    toml::from_str(&text).with_context(|| format!("reading {path}"))
}

/// Prints the link code for the server's key, making the key if missing.
fn print_link(config: &Config) -> Result<()> {
    let path = config
        .secret_key
        .as_deref()
        .context("set secret_key in the config, or the server's key changes on every start")?;
    let key = config::load_secret_key(Some(path))?;
    let name = config.instance.as_ref().map_or("Beep", |i| i.name.as_str());
    println!("{}", instance::link_code(&key, name, SystemTime::now())?);
    Ok(())
}

/// Live transcription through Mistral when the server has its key.
fn realtime(
    mistral: Option<&Arc<MistralProvider>>,
    config: &Config,
) -> Option<Arc<dyn RealtimeStt>> {
    let mistral = mistral?;
    Some(Arc::new(MistralStt::new(
        mistral.clone(),
        config.voice.realtime_model.clone(),
    )))
}

/// Daily log files kept before the oldest is deleted.
const LOG_FILES_KEPT: usize = 14;

/// Logs to stderr and a daily file; debug by default except html5ever and
/// iroh's networking crates. `RUST_LOG` overrides it.
fn start_logging(config: &Config) -> Result<()> {
    let dir = config.log_dir();
    let file = RollingFile::new(&dir.join("beep.log"), LOG_FILES_KEPT)?;
    logging::start(
        "debug,html5ever=info,iroh=warn,noq=warn,n0=warn,netwatch=warn,netdev=warn,portmapper=warn,igd_next=warn",
        true,
        Some(file),
    )?;
    log::info!("logging to {}", dir.display());
    Ok(())
}
