//! The machine's live resources, for `Up::Stats`: free disk where the
//! user's files live, memory in use and the load average.

use crate::fsat;
use anyhow::{Context, Result};
use beep_proto::node::{DiskUsage, StatsReport};
use std::path::{Path, PathBuf};
use tokio::task::spawn_blocking;

/// Samples the machine. `folders` are measured in order, each volume once:
/// the workspace, then the home folder.
pub async fn sample(folders: Vec<PathBuf>) -> StatsReport {
    // statvfs can hang on a stuck network mount, so it stays off the
    // async workers.
    let disks = match spawn_blocking(move || disks(&folders)).await {
        Ok(disks) => disks,
        Err(e) => {
            log::warn!("measuring the disks failed: {e}");
            Vec::new()
        }
    };
    let memory = memory().await.unwrap_or_else(|e| {
        log::debug!("reading memory use failed: {e:#}");
        Memory::default()
    });
    StatsReport {
        disks,
        memory_used_mb: memory.used_mb,
        memory_total_mb: memory.total_mb,
        load: load(),
    }
}

/// The volume of each folder, skipping a folder that is on the volume of
/// one before it or cannot be measured.
fn disks(folders: &[PathBuf]) -> Vec<DiskUsage> {
    let mut seen = Vec::new();
    let mut disks = Vec::new();
    for folder in folders {
        match disk(folder) {
            Ok((device, usage)) if !seen.contains(&device) => {
                seen.push(device);
                disks.push(usage);
            }
            Ok(_) => {}
            Err(e) => log::debug!("not measuring {}: {e:#}", folder.display()),
        }
    }
    disks
}

/// The volume's device id, so a second folder on it is skipped, and how
/// full it is.
fn disk(folder: &Path) -> Result<(u64, DiskUsage)> {
    let volume = fsat::volume(folder).context("reading the disk's size")?;
    let usage = DiskUsage {
        path: folder.display().to_string(),
        free_bytes: volume.free,
        total_bytes: volume.total,
    };
    Ok((volume.device, usage))
}

#[derive(Debug, Default, PartialEq)]
struct Memory {
    used_mb: u64,
    total_mb: u64,
}

#[cfg(unix)]
async fn memory() -> Result<Memory> {
    use crate::env;
    if cfg!(target_os = "macos") {
        let total_mb = env::memory_mb().await.context("hw.memsize")?;
        let vm_stat = env::output("vm_stat", &[]).await.context("vm_stat")?;
        let available_mb = vm_stat_available_mb(&vm_stat).context("vm_stat output")?;
        Ok(Memory {
            used_mb: total_mb.saturating_sub(available_mb),
            total_mb,
        })
    } else {
        let meminfo = tokio::fs::read_to_string("/proc/meminfo")
            .await
            .context("/proc/meminfo")?;
        meminfo_memory(&meminfo).context("/proc/meminfo has no MemTotal or MemAvailable")
    }
}

#[cfg(windows)]
async fn memory() -> Result<Memory> {
    use std::mem::size_of;
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX {
        dwLength: u32::try_from(size_of::<MEMORYSTATUSEX>())?,
        ..MEMORYSTATUSEX::default()
    };
    // SAFETY: `status` is the struct the call fills, with its length set
    // as the call requires.
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return Err(std::io::Error::last_os_error()).context("GlobalMemoryStatusEx");
    }
    let mb = 1024 * 1024;
    Ok(Memory {
        used_mb: status.ullTotalPhys.saturating_sub(status.ullAvailPhys) / mb,
        total_mb: status.ullTotalPhys / mb,
    })
}

#[cfg_attr(windows, allow(dead_code))]
/// Used and total memory from `/proc/meminfo`, which gives kB. Used is
/// what the kernel cannot hand out without swapping.
fn meminfo_memory(meminfo: &str) -> Option<Memory> {
    let field = |name: &str| -> Option<u64> {
        let line = meminfo.lines().find(|line| line.starts_with(name))?;
        line[name.len()..].split_whitespace().next()?.parse().ok()
    };
    let total_kb = field("MemTotal:")?;
    let available_kb = field("MemAvailable:")?;
    Some(Memory {
        used_mb: total_kb.saturating_sub(available_kb) / 1024,
        total_mb: total_kb / 1024,
    })
}

#[cfg_attr(windows, allow(dead_code))]
/// Free, inactive and speculative pages from `vm_stat`, in MB: what macOS
/// can hand out without compressing or swapping.
fn vm_stat_available_mb(vm_stat: &str) -> Option<u64> {
    let page_size: u64 = vm_stat
        .split("page size of ")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    let pages = |name: &str| -> Option<u64> {
        let line = vm_stat.lines().find(|line| line.starts_with(name))?;
        line[name.len()..].trim().trim_end_matches('.').parse().ok()
    };
    let available =
        pages("Pages free:")? + pages("Pages inactive:")? + pages("Pages speculative:")?;
    Some(available.saturating_mul(page_size) / (1024 * 1024))
}

/// The load average over the last minute.
#[cfg(unix)]
fn load() -> Option<f64> {
    let mut averages = [0f64; 3];
    // SAFETY: `averages` has room for the 3 values asked for, and
    // getloadavg writes no more than it is given.
    let got = unsafe { libc::getloadavg(averages.as_mut_ptr(), 3) };
    (got >= 1).then_some(averages[0])
}

/// Windows keeps no load average.
#[cfg(windows)]
fn load() -> Option<f64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_each_volume_once() {
        let dir = tempfile::tempdir().unwrap();
        let inner = dir.path().join("inner");
        std::fs::create_dir(&inner).unwrap();
        let missing = dir.path().join("missing");
        let disks = disks(&[dir.path().to_path_buf(), missing, inner]);
        assert_eq!(disks.len(), 1);
        assert_eq!(disks[0].path, dir.path().display().to_string());
        assert!(disks[0].total_bytes > 0);
        assert!(disks[0].free_bytes <= disks[0].total_bytes);
    }
}
