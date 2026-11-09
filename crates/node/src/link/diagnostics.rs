//! Periodic local snapshots of transport and application metrics.

use beep_proto::logging::RollingFile;
use iroh::Endpoint;
use iroh_metrics::{MetricsSource, Registry};
use metrics_exporter_prometheus::PrometheusBuilder;
use std::{
    io::Write,
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio_util::task::AbortOnDropHandle;

/// Records one snapshot per minute, retaining seven rotated daily files.
/// The returned handle owns the task and must be kept until shutdown.
pub fn start(endpoint: &Endpoint, path: &Path) -> anyhow::Result<AbortOnDropHandle<()>> {
    let file = Arc::new(RollingFile::new(path, 7)?);
    let handle = PrometheusBuilder::new().install_recorder()?;
    let mut registry = Registry::default();
    registry.register_all(endpoint.metrics());
    Ok(AbortOnDropHandle::new(tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                scheduled = tick.tick() => {
                    let delay = tokio::time::Instant::now().saturating_duration_since(scheduled);
                    metrics::histogram!("beep_runtime_tick_delay_seconds").record(delay.as_secs_f64());

                    if delay >= Duration::from_secs(1) {
                        log::warn!("runtime timer delayed {} ms", delay.as_millis());
                    }
                    continue;
                }
                _ = interval.tick() => {}
            }
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let transport = match registry.encode_openmetrics_to_string() {
                Ok(text) => text,
                Err(e) => {
                    log::warn!("encoding transport metrics: {e}");
                    continue;
                }
            };
            handle.run_upkeep();
            let application = handle.render();
            let file = file.clone();
            match tokio::task::spawn_blocking(move || {
                let mut file = &*file;
                let snapshot = format!(
                    "# snapshot unix_seconds={timestamp}\n{transport}{application}\n# end snapshot\n"
                );
                file.write_all(snapshot.as_bytes())?;
                file.flush()
            })
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(e)) => log::warn!("writing metrics: {e}"),
                Err(e) => log::warn!("metrics writer failed: {e}"),
            }
        }
    })))
}

#[cfg(test)]
#[path = "../tests/link_diagnostics.rs"]
mod tests;
