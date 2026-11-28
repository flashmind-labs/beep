use anyhow::{Context, Result};
use beep_proto::logging::{self, RollingFile};
use beep_server::{
    config::Config,
    reports::{self, Reports},
};
use std::{env, fs, net::SocketAddr, sync::Arc};

/// `beep-server [config]` receives problem reports.
#[tokio::main]
async fn main() -> Result<()> {
    let path = env::args().nth(1).unwrap_or_else(|| "config.toml".into());
    let text = fs::read_to_string(&path).with_context(|| format!("reading {path}"))?;
    let config: Config = toml::from_str(&text).with_context(|| format!("reading {path}"))?;
    start_logging(&config)?;

    let reports = Reports::new(&config.reports, beep_proto::https::client().build()?)?;
    let listen = config.http_listen();
    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .with_context(|| format!("binding the HTTP on {listen}"))?;
    log::info!("receiving reports; http={listen}");

    // The peer's address, which the reports route reads.
    axum::serve(
        listener,
        reports::routes(Arc::new(reports)).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(stop_signal())
    .await?;
    Ok(())
}

async fn stop_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            Err(e) => {
                log::warn!("no SIGTERM handler: {e}");
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;

    log::info!("stopping");
}

/// Daily log files kept before the oldest is deleted.
const LOG_FILES_KEPT: usize = 14;

/// Logs to stderr and a daily file. `RUST_LOG` overrides the level.
fn start_logging(config: &Config) -> Result<()> {
    let dir = config.log_dir();
    let file = RollingFile::new(&dir.join("beep.log"), LOG_FILES_KEPT)?;
    logging::start("debug,iroh=warn", true, Some(file))?;
    log::info!("logging to {}", dir.display());
    Ok(())
}
