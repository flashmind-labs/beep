use anyhow::{Context, Result};
use beep_server::{
    apns::Apns,
    app::{App, ServerTools, admin},
    apps::ComposioSettings,
    config::{self, Config},
    decision::DecisionTool,
    fcm::Firebase,
    memories,
    metering::Metered,
    models::ModelChoices,
    nodes,
    push::Pusher,
    remote::{Fallback, RemotePush},
    sandbox::Sandbox,
    search::SearchTool,
    store::Store,
    stt::{MistralStt, RealtimeStt},
    tools::Transitous,
    transport,
};
use flashmind::{
    llm::{MistralProvider, OpenRouterProvider},
    types::{AgentLlmConfig, LlmProvider, Model},
};
use iroh::{Endpoint, endpoint::presets, protocol::Router};
use logforth::{
    append::{Stderr, file::FileBuilder},
    filter::rustlog::RustLogFilterBuilder,
};
use secrecy::SecretString;
use std::{env, fs, num::NonZeroUsize, path::Path, sync::Arc};

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = env::args().nth(1).unwrap_or_else(|| "config.toml".into());
    let config: Config = toml::from_str(
        &fs::read_to_string(&config_path).with_context(|| format!("reading {config_path}"))?,
    )?;
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
    .with_auto(config.auto_models.clone())
    .with_agents(config.agents.clone())
    .with_research(config.research.clone());

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
            compaction_model: config.compaction_model.clone(),
            sandbox,
            transitous: Some(Arc::new(Transitous::new()?)),
            voice: config.voice.clone(),
            realtime: realtime(mistral.as_ref(), &config),
            mistral: mistral.map(|m| m as Arc<dyn LlmProvider>),
        },
        models,
    );
    // Subscribed before users start, so notices from their first crons wait
    // for the pusher instead of being dropped.
    let notices = app.notifications.subscribe();
    app.start_users().await?;
    admin::serve(app.clone(), admin::default_path(Path::new(&config_path)))?;
    beep_server::compaction::start(app.clone());
    beep_server::titles::start(app.clone());
    beep_server::memories::start_purge(app.clone());
    beep_server::store::start_pruning(app.store.clone());
    beep_server::events::start(
        &app,
        beep_server::events::Settings {
            triage: config.events.triage_model.clone(),
            ..beep_server::events::Settings::default()
        },
    );
    beep_server::watchers::start(
        &app,
        beep_server::watchers::Settings::new(&config.watchers, app.llm()),
    );
    let cron_runner = beep_server::cron::start(app.clone(), &config.log_dir());
    tokio::spawn(async move {
        if let Err(e) = cron_runner.start().await {
            log::error!("cron runner failed: {e}");
        }
    });

    let secret_key = config::load_secret_key(config.secret_key.as_deref())?;
    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(secret_key)
        .bind()
        .await?;

    log::info!("iroh endpoint bound; node_id={}", endpoint.id());

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

/// Live transcription for `Transcribe`, through Mistral when the server has
/// its key. Without it the apps hear that live voice is unavailable.
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
