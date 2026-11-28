//! Bounded timings from the webviews, without page arguments or user data.

use crate::store::lock;
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use serde::Deserialize;
use std::{
    io::{self, Write},
    path::Path,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use std::{
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};
use tauri::AppHandle;
use tauri::Manager;
use tracing_appender::rolling::{RollingFileAppender, Rotation};

#[derive(Deserialize)]
pub(crate) struct Timing {
    kind: String,
    name: String,
    outcome: String,
    ms: f64,
}

const PAGES: &[&str] = &[
    "home",
    "projects",
    "pane",
    "agent",
    "agentnew",
    "agents",
    "terminals",
    "providers",
    "sync",
    "instructions",
    "machines",
    "settings",
    "starting",
    "joincode",
    "joining",
    "invite-phone",
];
const COMMANDS: &[&str] = &[
    "machine_clocks",
    "project_sync_detail",
    "agent_worktrees",
    "session_moves",
    "session_move_progress",
    "agent_view",
    "agent_older",
    "agent_modes",
    "agent_tasks",
    "agent_sessions",
    "agent_hosts",
    "center_home",
    "project_sync",
    "project_git_state",
    "center_project_lists",
    "center_project_runs",
];

#[tauri::command]
pub(crate) fn desktop_trace_enabled() -> bool {
    std::env::var_os("BEEP_DESKTOP_TRACE_COMMANDS").is_some_and(|value| value == "1")
}

fn labels(t: &Timing) -> Option<(&'static str, &'static str, &'static str)> {
    if !t.ms.is_finite() || !(0.0..=600_000.0).contains(&t.ms) {
        return None;
    }

    let outcome = match t.outcome.as_str() {
        "ok" => "ok",
        "error" => "error",
        "timeout" => "timeout",
        _ => return None,
    };
    let (kind, names) = match t.kind.as_str() {
        "page" => ("page_mount_painted", PAGES),
        "data" => ("data_ready", &["agent"][..]),
        "command" => ("command", COMMANDS),
        _ => return None,
    };
    let name = *names.iter().find(|&&name| name == t.name)?;
    Some((kind, name, outcome))
}

/// Records a small batch so timing a call never recursively times itself.
#[tauri::command]
pub(crate) fn desktop_timings(samples: Vec<Timing>) {
    for sample in samples.into_iter().take(200) {
        let Some((kind, name, outcome)) = labels(&sample) else {
            continue;
        };
        metrics::counter!("beep_desktop_timings_total", "kind" => kind, "name" => name, "outcome" => outcome).increment(1);
        metrics::histogram!("beep_desktop_duration_seconds", "kind" => kind, "name" => name, "outcome" => outcome).record(sample.ms / 1000.0);
        let slow = if kind == "page_mount_painted" {
            250.0
        } else {
            1000.0
        };

        if sample.ms >= slow {
            log::info!("desktop {kind} {name} took {:.0} ms ({outcome})", sample.ms);
        }
    }
}

/// A webview's UI samples since its last flush (#571), as fixed-bucket histograms
/// labelled with a fixed view, so no content, id, path or name can reach the metrics.
#[derive(Deserialize)]
pub(crate) struct UiBatch {
    window: UiWindow,
    histograms: Vec<UiHistogram>,
    shown_ms: Vec<UiShown>,
    hidden_ms: f64,
    slow: Vec<UiSlow>,
    supported: Vec<UiEntry>,
    overhead_ms: f64,
}

#[derive(Deserialize)]
struct UiHistogram {
    kind: UiKind,
    view: UiView,
    input: Option<UiInput>,
    counts: Vec<u32>,
    sum_ms: f64,
    max_ms: f64,
}

#[derive(Deserialize)]
struct UiShown {
    view: UiView,
    ms: f64,
}

#[derive(Deserialize)]
struct UiSlow {
    kind: UiKind,
    view: UiView,
    ms: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, strum::IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
enum UiWindow {
    Main,
    Bar,
    Phone,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, strum::IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
enum UiKind {
    Interaction,
    LongTask,
    Frame,
    Open,
    Fresh,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, strum::IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
enum UiView {
    Home,
    Session,
    New,
    Project,
    Diff,
    Picker,
    Settings,
    Bar,
    Terminals,
    List,
    Other,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, strum::IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
enum UiInput {
    Pointer,
    Key,
    Other,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, strum::IntoStaticStr)]
enum UiEntry {
    #[serde(rename = "event")]
    #[strum(serialize = "event")]
    Event,
    #[serde(rename = "longtask")]
    #[strum(serialize = "longtask")]
    LongTask,
    #[serde(rename = "long-animation-frame")]
    #[strum(serialize = "long_animation_frame")]
    LongAnimationFrame,
}

/// The webview's bucket bounds in ms, `PERF_BUCKETS_MS` in `ui/perf-state.ts`.
const UI_BUCKETS_MS: [f64; 10] = [
    16.0, 33.0, 50.0, 100.0, 200.0, 300.0, 500.0, 1000.0, 2000.0, 5000.0,
];
/// More samples than a minute of use can make; a batch beyond it is not real.
const UI_MAX_COUNT: u32 = 100_000;
/// The longest a sample or a flush interval's time can be.
const UI_MAX_MS: f64 = 86_400_000.0;
/// One slow line per kind at most this often.
const UI_SLOW_LOG_SECS: u64 = 30;
const ALL_ENTRIES: [UiEntry; 3] = [
    UiEntry::Event,
    UiEntry::LongTask,
    UiEntry::LongAnimationFrame,
];

impl UiHistogram {
    fn valid(&self) -> bool {
        self.counts.len() == UI_BUCKETS_MS.len() + 1
            && self.counts.iter().all(|count| *count <= UI_MAX_COUNT)
            && ui_ms(self.sum_ms)
            && ui_ms(self.max_ms)
            && (self.input.is_some() == (self.kind == UiKind::Interaction))
    }

    /// Each bucket's samples at the middle of its bounds; the open last bucket at the largest sample.
    fn values(&self) -> impl Iterator<Item = (f64, u32)> + '_ {
        self.counts
            .iter()
            .enumerate()
            .filter(|(_, count)| **count > 0)
            .map(|(index, count)| {
                let value = match (index.checked_sub(1), UI_BUCKETS_MS.get(index)) {
                    (None, Some(upper)) => upper / 2.0,
                    (Some(lower), Some(upper)) => (UI_BUCKETS_MS[lower] + upper) / 2.0,
                    _ => self.max_ms.max(UI_BUCKETS_MS[UI_BUCKETS_MS.len() - 1]),
                };
                (value, *count)
            })
    }

    /// Samples above the kind's slow threshold, counted from the buckets.
    fn slow(&self) -> u64 {
        let Some(threshold) = slow_ms(self.kind) else {
            return 0;
        };
        // Bucket `i` holds samples above bound `i - 1`.
        let first = UI_BUCKETS_MS.partition_point(|bound| *bound < threshold) + 1;
        self.counts
            .iter()
            .skip(first.min(self.counts.len()))
            .map(|count| u64::from(*count))
            .sum()
    }
}

fn ui_ms(ms: f64) -> bool {
    ms.is_finite() && (0.0..=UI_MAX_MS).contains(&ms)
}

/// Rounded to whole ms; `ui_ms` bounds it first.
fn whole_ms(ms: f64) -> u64 {
    // In 0..=UI_MAX_MS, so the cast neither truncates nor wraps.
    ms.round() as u64
}

fn slow_ms(kind: UiKind) -> Option<f64> {
    match kind {
        UiKind::Interaction => Some(500.0),
        UiKind::LongTask => Some(200.0),
        UiKind::Frame | UiKind::Open | UiKind::Fresh => None,
    }
}

/// Merges a webview's UI batch into the app's metrics, the ones `beep_metrics` reads.
#[tauri::command]
pub(crate) fn desktop_ui_metrics(_app: AppHandle, batch: UiBatch) {
    let started = Instant::now();
    record_ui(&batch);

    for slow in batch.slow.iter().take(8) {
        if ui_ms(slow.ms) && slow_log_due(slow.kind) {
            let kind: &'static str = slow.kind.into();
            let view: &'static str = slow.view.into();
            log::warn!("slow ui {kind} on {view}: {:.0} ms", slow.ms);
        }
    }

    metrics::histogram!("beep_desktop_ui_merge_seconds").record(started.elapsed().as_secs_f64());

    // A hosted desktop uses its node's snapshots; hostless apps write their own.
    if let Some(phone) = _app.try_state::<PhoneMetrics>() {
        phone.snapshot();
    }
}

/// How many days of a phone's metrics are kept.
const PHONE_DAYS: usize = 7;

/// Metrics for an app without an embedded node. Each UI batch writes a snapshot
/// in the node's format. No timer runs while the app is in the background.
pub(crate) struct PhoneMetrics {
    handle: PrometheusHandle,
    file: Arc<Mutex<RollingFileAppender>>,
}

impl PhoneMetrics {
    /// Installs the app's metrics recorder and writes its snapshots to `dir`.
    #[cfg_attr(
        desktop,
        allow(dead_code, reason = "the embedded node keeps the metrics")
    )]
    pub(crate) fn start(dir: &Path) -> anyhow::Result<Self> {
        Self::new(PrometheusBuilder::new().install_recorder()?, dir)
    }

    #[cfg_attr(
        desktop,
        allow(dead_code, reason = "the embedded node keeps the metrics")
    )]
    fn new(handle: PrometheusHandle, dir: &Path) -> anyhow::Result<Self> {
        let file = RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_prefix("metrics")
            .filename_suffix("log")
            .max_log_files(PHONE_DAYS)
            .build(dir)?;
        Ok(Self {
            handle,
            file: Arc::new(Mutex::new(file)),
        })
    }

    /// Writes one snapshot off the main thread.
    fn snapshot(&self) {
        let handle = self.handle.clone();
        let file = self.file.clone();
        tauri::async_runtime::spawn_blocking(move || {
            if let Err(e) = write_snapshot(&handle, &file) {
                log::warn!("writing metrics: {e}");
            }
        });
    }
}

fn write_snapshot(handle: &PrometheusHandle, file: &Mutex<RollingFileAppender>) -> io::Result<()> {
    handle.run_upkeep();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let snapshot = format!(
        "# snapshot unix_seconds={timestamp}\n{}\n# end snapshot\n",
        handle.render()
    );
    let mut file = lock(file);
    file.write_all(snapshot.as_bytes())?;
    file.flush()
}

fn record_ui(batch: &UiBatch) {
    let window: &'static str = batch.window.into();
    metrics::counter!("beep_desktop_ui_batches_total", "window" => window).increment(1);

    for histogram in batch.histograms.iter().take(256) {
        if !histogram.valid() {
            continue;
        }

        let kind: &'static str = histogram.kind.into();
        let view: &'static str = histogram.view.into();
        let input: &'static str = histogram.input.map_or("none", Into::into);
        let recorded = metrics::histogram!("beep_desktop_ui_seconds", "window" => window, "kind" => kind, "view" => view, "input" => input);

        for (ms, count) in histogram.values() {
            recorded.record_many(ms / 1000.0, usize::try_from(count).unwrap_or(0));
        }

        let slow = histogram.slow();
        if slow > 0 {
            metrics::counter!("beep_desktop_ui_slow_total", "window" => window, "kind" => kind, "view" => view).increment(slow);
        }
    }

    for shown in batch.shown_ms.iter().take(64) {
        if ui_ms(shown.ms) {
            let view: &'static str = shown.view.into();
            metrics::counter!("beep_desktop_ui_shown_milliseconds_total", "window" => window, "view" => view).increment(whole_ms(shown.ms));
        }
    }

    if ui_ms(batch.hidden_ms) {
        metrics::counter!("beep_desktop_ui_hidden_milliseconds_total", "window" => window)
            .increment(whole_ms(batch.hidden_ms));
    }

    if ui_ms(batch.overhead_ms) {
        metrics::counter!("beep_desktop_ui_overhead_milliseconds_total", "window" => window)
            .increment(whole_ms(batch.overhead_ms));
    }

    for entry in ALL_ENTRIES {
        let name: &'static str = entry.into();
        metrics::gauge!("beep_desktop_ui_observed", "window" => window, "entry" => name)
            .set(f64::from(batch.supported.contains(&entry)));
    }
}

/// True at most once per `UI_SLOW_LOG_SECS` for each kind.
fn slow_log_due(kind: UiKind) -> bool {
    static START: OnceLock<Instant> = OnceLock::new();
    static LAST: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];
    let start = *START.get_or_init(Instant::now);
    // Seconds since the first call, plus one so the first line is never held back.
    let now = start.elapsed().as_secs() + UI_SLOW_LOG_SECS + 1;
    let last = &LAST[kind as usize];
    let previous = last.load(Ordering::Relaxed);
    now.saturating_sub(previous) >= UI_SLOW_LOG_SECS
        && last
            .compare_exchange(previous, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
}

#[cfg(test)]
#[path = "../tests/platform_metrics.rs"]
mod tests;
