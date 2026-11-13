//! How long a launch takes, stage by stage, in the log: from the process
//! starting to the page's first draw of Home. Each line reads
//! `startup: <stage> at <n> ms`, counted from the process start.

use std::{
    collections::BTreeMap,
    sync::OnceLock,
    time::{Duration, SystemTime},
};

static STARTED: OnceLock<SystemTime> = OnceLock::new();
/// A window that opens later than this after the process started is not part of the launch.
const LAUNCH: Duration = Duration::from_secs(10);

/// Logs that the launch reached `stage`.
pub(crate) fn stage(stage: &str) {
    log_stage(stage, SystemTime::now());
}

/// The page's own stages: when each happened, in milliseconds since the Unix
/// epoch. Sent once, after the window first drew Home.
#[tauri::command]
pub fn startup_stages(window: tauri::Window, stages: BTreeMap<String, f64>) {
    let mut stages: Vec<(String, f64)> = stages
        .into_iter()
        .filter(|(_, at)| at.is_finite() && *at > 0.0)
        .collect();
    stages.sort_by(|a, b| a.1.total_cmp(&b.1));

    for (stage, at) in stages {
        let Ok(since_epoch) = Duration::try_from_secs_f64(at / 1000.0) else {
            continue;
        };
        let at = SystemTime::UNIX_EPOCH + since_epoch;

        if since_start(at) > LAUNCH {
            return;
        }

        log_stage(&format!("{} {stage}", window.label()), at);
    }
}

fn log_stage(stage: &str, at: SystemTime) {
    log::info!("startup: {stage} at {} ms", since_start(at).as_millis());
}

fn since_start(at: SystemTime) -> Duration {
    let started = *STARTED.get_or_init(|| process_started().unwrap_or(at));
    at.duration_since(started).unwrap_or(Duration::ZERO)
}

/// When the kernel started this process, which counts the time the binary
/// takes to load before `main`.
#[cfg(target_os = "macos")]
fn process_started() -> Option<SystemTime> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let size = libc::c_int::try_from(size_of::<libc::proc_bsdinfo>()).ok()?;
    let pid = libc::pid_t::try_from(std::process::id()).ok()?;
    // SAFETY: `info` is a buffer of `size` bytes that `proc_pidinfo` fills
    // with a `proc_bsdinfo` when it returns `size`.
    let info = unsafe {
        if libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        ) != size
        {
            return None;
        }

        info.assume_init()
    };
    let since_epoch =
        Duration::from_secs(info.pbi_start_tvsec) + Duration::from_micros(info.pbi_start_tvusec);
    Some(SystemTime::UNIX_EPOCH + since_epoch)
}

#[cfg(not(target_os = "macos"))]
fn process_started() -> Option<SystemTime> {
    None
}
