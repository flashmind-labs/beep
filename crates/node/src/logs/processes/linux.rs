use super::process::Process;
use std::{
    collections::HashMap,
    fs, io,
    time::{Duration, Instant},
};

fn timing() -> io::Result<(u64, u64)> {
    let stat = fs::read_to_string("/proc/stat")?;
    let boot = stat
        .lines()
        .find_map(|l| l.strip_prefix("btime "))
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| io::Error::other("missing boot time"))?;
    // SAFETY: sysconf reads a system constant and takes no pointers.
    let ticks = u64::try_from(unsafe { libc::sysconf(libc::_SC_CLK_TCK) })
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| io::Error::other("invalid clock ticks"))?;
    Ok((boot, ticks))
}

fn read(pid: u32, boot: u64, ticks: u64) -> Option<Process> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, fields) = stat.rsplit_once(") ")?;
    let fields: Vec<_> = fields.split_whitespace().collect();

    if matches!(*fields.first()?, "Z" | "X") {
        return None;
    }

    let birth: u64 = fields.get(19)?.parse().ok()?;
    let cmd = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let command = String::from_utf8_lossy(&cmd)
        .replace('\0', " ")
        .trim()
        .to_owned();
    Some(Process {
        pid,
        parent: fields.get(1)?.parse().ok()?,
        group: fields.get(2)?.parse().ok()?,
        birth,
        started_at: boot.checked_add(birth / ticks)?,
        command: command.into(),
    })
}

fn snapshot_blocking() -> io::Result<HashMap<u32, Process>> {
    let (boot, ticks) = timing()?;
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut all = HashMap::new();

    for entry in fs::read_dir("/proc")?.flatten() {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "process listing timed out",
            ));
        }

        let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse().ok()) else {
            continue;
        };
        if let Some(p) = read(pid, boot, ticks) {
            all.insert(pid, p);
        }
    }

    Ok(all)
}

pub(crate) fn alive(p: &Process) -> bool {
    read(p.pid, 0, 1).is_some_and(|now| now.birth == p.birth)
}

pub(crate) async fn ports(processes: &[Process]) -> HashMap<u32, Vec<u16>> {
    let pids: Vec<_> = processes.iter().map(|p| p.pid).collect();
    tokio::task::spawn_blocking(move || {
        let deadline = Instant::now() + Duration::from_millis(500);
        let mut sockets = HashMap::new();

        for path in ["/proc/net/tcp", "/proc/net/tcp6"] {
            let Ok(net) = fs::read_to_string(path) else {
                continue;
            };

            for line in net.lines().skip(1) {
                if Instant::now() >= deadline {
                    break;
                }

                let f: Vec<_> = line.split_whitespace().collect();

                if f.get(3) != Some(&"0A") {
                    continue;
                }

                if let Some((inode, port)) = f.get(9).and_then(|s| s.parse::<u64>().ok()).zip(
                    f.get(1)
                        .and_then(|s| s.rsplit_once(':'))
                        .and_then(|(_, s)| u16::from_str_radix(s, 16).ok()),
                ) {
                    sockets.insert(inode, port);
                }
            }
        }

        let mut result = HashMap::new();

        for pid in pids {
            if Instant::now() >= deadline {
                break;
            }

            let Ok(fds) = fs::read_dir(format!("/proc/{pid}/fd")) else {
                continue;
            };
            let mut ports = Vec::new();

            for fd in fds.flatten() {
                if Instant::now() >= deadline {
                    break;
                }

                let Ok(link) = fs::read_link(fd.path()) else {
                    continue;
                };
                let text = link.to_string_lossy();

                if let Some(port) = text
                    .strip_prefix("socket:[")
                    .and_then(|s| s.strip_suffix(']'))
                    .and_then(|s| s.parse::<u64>().ok())
                    .and_then(|inode| sockets.get(&inode))
                {
                    ports.push(*port);
                }
            }

            ports.sort_unstable();
            ports.dedup();
            result.insert(pid, ports);
        }

        result
    })
    .await
    .unwrap_or_default()
}

pub(crate) async fn snapshot() -> anyhow::Result<HashMap<u32, Process>> {
    Ok(tokio::task::spawn_blocking(snapshot_blocking).await??)
}

#[cfg(test)]
#[path = "../../tests/logs_processes_linux.rs"]
mod tests;
