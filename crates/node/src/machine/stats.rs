//! The machine's live resources, for `Report::Stats`: free disk where the
//! user's files live and on its other disks, memory in use and the load
//! average.

use crate::files::fsat;
use anyhow::{Context, Result};
use beep_proto::node::{DiskUsage, StatsReport};
use std::path::{Path, PathBuf};
use tokio::task::spawn_blocking;

/// Samples the machine. `folders` are measured in order, each volume once:
/// the workspace, then the home folder, then the machine's other disks.
pub async fn sample(mut folders: Vec<PathBuf>) -> StatsReport {
    // statvfs can hang on a stuck network mount, so it stays off the
    // async workers.
    let disks = match spawn_blocking(move || {
        folders.extend(other_volumes());
        disks(&folders)
    })
    .await
    {
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

/// The machine's fixed drives.
#[cfg(windows)]
fn other_volumes() -> Vec<PathBuf> {
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    use windows_sys::Win32::System::WindowsProgramming::DRIVE_FIXED;
    // SAFETY: the call takes nothing and returns a bitmask of drive letters.
    let drives = unsafe { GetLogicalDrives() };
    (0u8..26)
        .filter(|letter| drives & (1 << letter) != 0)
        .map(|letter| PathBuf::from(format!("{}:\\", char::from(b'A' + letter))))
        .filter(|root| {
            let wide: Vec<u16> = root.as_os_str().encode_wide().chain(once(0)).collect();
            // SAFETY: `wide` is a NUL-terminated path that outlives the call.
            unsafe { GetDriveTypeW(wide.as_ptr()) == DRIVE_FIXED }
        })
        .collect()
}

/// The machine's mounted disks. Listing them reads no disk, so a stuck
/// network mount cannot hold this up.
#[cfg(unix)]
fn other_volumes() -> Vec<PathBuf> {
    use std::fs;
    use std::process::Command;
    let listed = if cfg!(target_os = "macos") {
        Command::new("mount")
            .output()
            .map(|out| mac_volumes(&String::from_utf8_lossy(&out.stdout)))
    } else {
        fs::read_to_string("/proc/self/mounts").map(|mounts| linux_volumes(&mounts))
    };

    match listed {
        // A container mounts single files too, such as /etc/hosts.
        Ok(listed) => listed.into_iter().filter(|path| path.is_dir()).collect(),
        Err(e) => {
            log::debug!("listing the disks failed: {e}");
            Vec::new()
        }
    }
}

#[cfg_attr(windows, allow(dead_code))]
/// The writable volumes in `/proc/self/mounts` that sit on a real device,
/// without the boot partitions. Spaces in a mount point come escaped.
fn linux_volumes(mounts: &str) -> Vec<PathBuf> {
    mounts
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let (source, point) = (fields.next()?, fields.next()?);
            let options = fields.nth(1)?;
            let real = source.starts_with("/dev/") && !source.starts_with("/dev/loop");
            let writable = !options.split(',').any(|option| option == "ro");
            (real && writable && !point.starts_with("/boot"))
                .then(|| PathBuf::from(point.replace("\\040", " ")))
        })
        .collect()
}

#[cfg_attr(windows, allow(dead_code))]
/// The local, writable volumes in `mount`'s output that are under
/// `/Volumes`, which leaves out disk images and network shares.
fn mac_volumes(mount: &str) -> Vec<PathBuf> {
    mount
        .lines()
        .filter_map(|line| {
            let (mounted, options) = line.rsplit_once(" (")?;
            let (_, point) = mounted.split_once(" on ")?;
            let has = |name: &str| options.trim_end_matches(')').split(", ").any(|o| o == name);
            (point.starts_with("/Volumes/") && has("local") && !has("read-only"))
                .then(|| PathBuf::from(point))
        })
        .collect()
}

#[derive(Debug, Default, PartialEq)]
pub(super) struct Memory {
    used_mb: u64,
    total_mb: u64,
}

#[cfg(unix)]
async fn memory() -> Result<Memory> {
    use crate::machine::env;
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
    let (used_mb, total_mb) = crate::machine::env::windows_memory_mb()?;

    Ok(Memory { used_mb, total_mb })
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
#[cfg(all(unix, not(target_os = "android")))]
fn load() -> Option<f64> {
    let mut averages = [0f64; 3];
    // SAFETY: `averages` has room for the 3 values asked for, and
    // getloadavg writes no more than it is given.
    let got = unsafe { libc::getloadavg(averages.as_mut_ptr(), 3) };
    (got >= 1).then_some(averages[0])
}

/// Windows and Android keep no load average to read.
#[cfg(any(windows, target_os = "android"))]
fn load() -> Option<f64> {
    None
}

#[cfg(test)]
#[path = "../tests/machine_stats.rs"]
mod tests;
