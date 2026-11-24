use super::*;
use metrics_exporter_prometheus::PrometheusBuilder;
use std::sync::{Arc, Mutex};

#[derive(strum::IntoStaticStr)]
enum Msg {
    Slow,
}

/// The value of the metric line that starts with `prefix`.
fn value(text: &str, prefix: &str) -> f64 {
    text.lines()
        .find(|line| line.starts_with(prefix))
        .and_then(|line| line.rsplit_once(' '))
        .and_then(|(_, value)| value.parse().ok())
        .unwrap_or_else(|| panic!("no {prefix} in\n{text}"))
}

/// What is logged at `INFO` and above, while the guard lives.
#[derive(Clone, Default)]
struct Logged(Arc<Mutex<Vec<u8>>>);

impl Logged {
    fn capture(&self) -> tracing::subscriber::DefaultGuard {
        let writer = self.clone();
        tracing::subscriber::set_default(
            tracing_subscriber::fmt()
                .with_max_level(tracing::Level::INFO)
                .with_ansi(false)
                .with_writer(move || writer.clone())
                .finish(),
        )
    }

    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

impl std::io::Write for Logged {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().write(bytes)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test(start_paused = true)]
async fn waiting_and_handling_are_recorded_per_actor() {
    let recorder = PrometheusBuilder::new().build_recorder();
    let handle = recorder.handle();
    let _guard = metrics::set_default_local_recorder(&recorder);
    let (tx, mut rx) = channel::<Msg>("test", 4);

    tx.send(Msg::Slow).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (_, timing) = rx.recv().await.unwrap();
    tokio::time::sleep(Duration::from_millis(80)).await;
    drop(timing);

    let text = handle.render();
    let actor = r#"{actor="test"}"#;
    assert!((value(&text, &format!("beep_actor_wait_seconds_sum{actor}")) - 0.2).abs() < 1e-9);
    assert!((value(&text, &format!("beep_actor_handle_seconds_sum{actor}")) - 0.08).abs() < 1e-9);
    assert!(!text.contains("message="), "{text}");
}

#[tokio::test(start_paused = true)]
async fn a_job_records_how_long_it_took() {
    let recorder = PrometheusBuilder::new().build_recorder();
    let handle = recorder.handle();
    let _guard = metrics::set_default_local_recorder(&recorder);

    let answer = job("test", "scan", async {
        tokio::time::sleep(Duration::from_millis(1_500)).await;
        7
    })
    .await;

    assert_eq!(answer, 7);
    let text = handle.render();
    let sum = value(&text, r#"beep_actor_job_seconds_sum{actor="test",job="scan"}"#);
    assert!((sum - 1.5).abs() < 1e-9, "{sum}");
}
