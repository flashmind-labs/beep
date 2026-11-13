//! Periodic local snapshots of transport and application metrics.

use beep_proto::logging::RollingFile;
use iroh::Endpoint;
use iroh_metrics::{MetricsSource, Registry};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::{
    io::Write,
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio_util::task::AbortOnDropHandle;

const SNAPSHOT_EVERY: Duration = Duration::from_secs(60);

/// Notes how late the runtime woke for a snapshot. The snapshot tick doubles
/// as the lag probe, so an idle machine wakes once a minute, not every second.
fn lagged(delay: Duration) {
    metrics::histogram!("beep_runtime_tick_delay_seconds").record(delay.as_secs_f64());

    if delay >= Duration::from_secs(1) {
        log::warn!("runtime timer delayed {} ms", delay.as_millis());
    }
}

/// Installs the recorder the application's metrics go to; what was recorded
/// before it is lost. The recorder's clock calibrates as it is made, for
/// 200 ms on Apple silicon, so an app with a window to open calls this on a
/// thread of its own.
pub fn install() -> anyhow::Result<PrometheusHandle> {
    Ok(PrometheusBuilder::new().install_recorder()?)
}

/// Records one snapshot of `handle`'s metrics and the endpoint's per minute,
/// retaining seven rotated daily files. The returned handle owns the task and
/// must be kept until shutdown.
pub fn start(
    handle: PrometheusHandle,
    endpoint: &Endpoint,
    path: &Path,
) -> anyhow::Result<AbortOnDropHandle<()>> {
    let file = Arc::new(RollingFile::new(path, 7)?);
    let mut registry = Registry::default();
    registry.register_all(endpoint.metrics());
    Ok(AbortOnDropHandle::new(tokio::spawn(async move {
        let mut interval = tokio::time::interval(SNAPSHOT_EVERY);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            let scheduled = interval.tick().await;
            lagged(tokio::time::Instant::now().saturating_duration_since(scheduled));

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
