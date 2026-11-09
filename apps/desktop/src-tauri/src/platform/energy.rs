//! Resource proxies for battery profiling. Battery power belongs to the whole device.

use std::{
    collections::HashMap,
    time::{Duration, Instant, SystemTime},
};

use battery::{
    Manager as BatteryManager, State,
    units::{energy::joule, power::watt},
};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tauri::{AppHandle, Manager};

const PERIOD: Duration = Duration::from_secs(60);

pub(crate) struct Sampling(tauri::async_runtime::JoinHandle<()>);

impl Sampling {
    pub(crate) fn stop(&self) {
        self.0.abort();
    }
}

impl Drop for Sampling {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(crate) fn start(app: AppHandle) -> Option<Sampling> {
    if std::env::var_os("BEEP_DESKTOP_RESOURCE_METRICS").is_some_and(|value| value == "0") {
        return None;
    }

    Some(Sampling(tauri::async_runtime::spawn(async move {
        let mut sampler = Sampler::default();
        let mut interval = tokio::time::interval(PERIOD);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            interval.tick().await;
            let context = context(&app);
            let result = tauri::async_runtime::spawn_blocking(move || {
                sampler.sample(context);
                sampler
            })
            .await;

            match result {
                Ok(next) => sampler = next,
                Err(error) => {
                    log::warn!("resource sampling stopped: {error}");
                    break;
                }
            }
        }
    })))
}

fn context(app: &AppHandle) -> Context {
    let mut visible = false;

    for window in app.webview_windows().into_values() {
        if window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(false) {
            visible = true;

            if window.is_focused().unwrap_or(false) {
                return Context::Foreground;
            }
        }
    }

    if visible {
        Context::Visible
    } else {
        Context::Hidden
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
enum Context {
    Foreground,
    Visible,
    Hidden,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
enum PowerSource {
    Battery,
    External,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
enum Role {
    Native,
    Webview,
    Workload,
}

#[derive(Clone, Copy, Default)]
struct Usage {
    cpu_ms: u64,
    read: u64,
    written: u64,
}

struct ProcessSample {
    pid: Pid,
    parent: Option<Pid>,
    started: u64,
    webview: bool,
    memory: u64,
    usage: Usage,
}

#[derive(Default)]
struct Sampler {
    system: System,
    previous: HashMap<(Pid, u64), Usage>,
    sampled_at: Option<SystemTime>,
    previous_context: Option<(Context, PowerSource)>,
}

impl Sampler {
    fn continuous(&self, now: SystemTime, context: Context, power: PowerSource) -> bool {
        self.sampled_at.is_some_and(|last| {
            now.duration_since(last)
                .is_ok_and(|elapsed| elapsed < PERIOD * 2)
        }) && self.previous_context == Some((context, power))
    }

    fn sample(&mut self, context: Context) {
        let started = Instant::now();
        // Do not collect command lines, environment, users, or executable paths.
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().without_tasks(),
        );
        let mut processes: Vec<_> = self
            .system
            .processes()
            .iter()
            .map(|(&pid, process)| {
                let disk = process.disk_usage();
                ProcessSample {
                    pid,
                    parent: process.parent(),
                    started: process.start_time(),
                    webview: is_webview(&process.name().to_string_lossy()),
                    memory: process.memory(),
                    usage: Usage {
                        cpu_ms: process.accumulated_cpu_time(),
                        read: disk.total_read_bytes,
                        written: disk.total_written_bytes,
                    },
                }
            })
            .collect();
        let roles = roles(&processes, Pid::from_u32(std::process::id()));
        // Enumerate parents first; expensive counters are read only for this app's tree.
        let pids: Vec<_> = roles.keys().copied().collect();
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pids),
            true,
            ProcessRefreshKind::nothing()
                .without_tasks()
                .with_cpu()
                .with_memory()
                .with_disk_usage(),
        );
        processes.retain(|process| {
            self.system
                .process(process.pid)
                .is_some_and(|current| current.start_time() == process.started)
        });

        for process in &mut processes {
            if let Some(current) = self.system.process(process.pid) {
                let disk = current.disk_usage();
                process.memory = current.memory();
                process.usage = Usage {
                    cpu_ms: current.accumulated_cpu_time(),
                    read: disk.total_read_bytes,
                    written: disk.total_written_bytes,
                };
            }
        }

        let now = SystemTime::now();
        // Resume gaps cannot describe one steady workload. Start a fresh baseline.
        let power = record_battery();
        let continuous = self.continuous(now, context, power);
        let mut next = HashMap::new();

        for role in [Role::Native, Role::Webview, Role::Workload] {
            let mut memory = 0u64;
            let mut count = 0u64;
            let mut cpu_ms = 0u64;
            let mut read = 0u64;
            let mut written = 0u64;

            for process in &processes {
                if roles.get(&process.pid) != Some(&role) {
                    continue;
                }
                count += 1;
                memory = memory.saturating_add(process.memory);
                let key = (process.pid, process.started);

                if continuous && let Some(previous) = self.previous.get(&key) {
                    let delta = delta(process.usage, *previous);
                    cpu_ms = cpu_ms.saturating_add(delta.cpu_ms);
                    read = read.saturating_add(delta.read);
                    written = written.saturating_add(delta.written);
                }

                next.insert(key, process.usage);
            }

            let role: &'static str = role.into();
            metrics::gauge!("beep_desktop_processes", "role" => role).set(count as f64);
            metrics::gauge!("beep_desktop_memory_bytes", "role" => role).set(memory as f64);

            if continuous {
                let context: &'static str = context.into();
                let power: &'static str = power.into();
                metrics::counter!("beep_desktop_cpu_milliseconds_total", "role" => role, "context" => context, "power" => power).increment(cpu_ms);
                metrics::counter!("beep_desktop_disk_read_bytes_total", "role" => role, "context" => context, "power" => power).increment(read);
                metrics::counter!("beep_desktop_disk_written_bytes_total", "role" => role, "context" => context, "power" => power).increment(written);
            }
        }

        self.previous = next;
        self.sampled_at = Some(now);
        self.previous_context = Some((context, power));
        metrics::histogram!("beep_desktop_resource_sample_seconds")
            .record(started.elapsed().as_secs_f64());
    }
}

fn delta(current: Usage, previous: Usage) -> Usage {
    Usage {
        cpu_ms: current.cpu_ms.saturating_sub(previous.cpu_ms),
        read: current.read.saturating_sub(previous.read),
        written: current.written.saturating_sub(previous.written),
    }
}

fn roles(processes: &[ProcessSample], root: Pid) -> HashMap<Pid, Role> {
    let mut roles = HashMap::from([(root, Role::Native)]);

    loop {
        let before = roles.len();

        for process in processes {
            if roles.contains_key(&process.pid) {
                continue;
            }

            if let Some(parent) = process.parent.and_then(|pid| roles.get(&pid)) {
                let role =
                    if *parent == Role::Webview || (*parent == Role::Native && process.webview) {
                        Role::Webview
                    } else {
                        Role::Workload
                    };
                roles.insert(process.pid, role);
            }
        }

        if roles.len() == before {
            return roles;
        }
    }
}

fn is_webview(name: &str) -> bool {
    name.starts_with("WebKit")
        || name.starts_with("com.apple.WebKit.")
        || name.eq_ignore_ascii_case("msedgewebview2.exe")
}

fn record_battery() -> PowerSource {
    let batteries = BatteryManager::new().and_then(|manager| manager.batteries());
    let Ok(batteries) = batteries else {
        metrics::gauge!("beep_desktop_battery_available").set(0.0);
        return PowerSource::Unknown;
    };
    let mut count = 0;
    let mut discharging = false;
    let mut unknown = false;
    let mut energy = 0.0;
    let mut discharge = 0.0;

    for battery in batteries {
        let Ok(battery) = battery else {
            unknown = true;
            continue;
        };
        count += 1;
        energy += f64::from(battery.energy().get::<joule>());

        match battery.state() {
            State::Discharging => {
                discharging = true;
                discharge += f64::from(battery.energy_rate().get::<watt>());
            }
            State::Charging | State::Full => {}
            _ => unknown = true,
        }
    }

    let available = count > 0 && !unknown && energy.is_finite() && discharge.is_finite();
    metrics::gauge!("beep_desktop_battery_available").set(f64::from(available));

    if !available {
        return PowerSource::Unknown;
    }

    metrics::gauge!("beep_desktop_device_battery_energy_joules").set(energy);
    metrics::gauge!("beep_desktop_device_battery_discharge_watts").set(discharge);
    if discharging {
        PowerSource::Battery
    } else {
        PowerSource::External
    }
}

#[cfg(test)]
#[path = "../tests/platform_energy.rs"]
mod tests;
