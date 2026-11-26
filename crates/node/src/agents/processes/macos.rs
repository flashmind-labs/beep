use super::process::Process;
use std::{
    collections::HashMap,
    mem::{MaybeUninit, size_of},
    time::Duration,
};
use tokio::{process::Command, time::timeout};

fn info(pid: u32) -> Option<libc::proc_bsdinfo> {
    let pid = libc::pid_t::try_from(pid).ok()?;
    let mut info = MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let size = i32::try_from(size_of::<libc::proc_bsdinfo>()).ok()?;
    // SAFETY: info points to a writable buffer of the specified size. The
    // buffer is read only when the kernel reports it wrote the whole struct.
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };

    if read != size {
        return None;
    }
    // SAFETY: the kernel initialized the whole struct above.
    let info = unsafe { info.assume_init() };
    (info.pbi_status != 5).then_some(info)
}

fn birth(info: &libc::proc_bsdinfo) -> u64 {
    info.pbi_start_tvsec
        .saturating_mul(1_000_000)
        .saturating_add(info.pbi_start_tvusec)
}

pub(super) async fn snapshot() -> anyhow::Result<HashMap<u32, Process>> {
    crate::run::process::counted("ps".as_ref());
    let output = timeout(
        Duration::from_secs(2),
        Command::new("ps")
            .args(["-wwaxo", "pid=,command="])
            .kill_on_drop(true)
            .output(),
    )
    .await??;

    if !output.status.success() {
        anyhow::bail!("ps could not list processes");
    }

    let mut all = HashMap::new();

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((pid, command)) = line.trim().split_once(char::is_whitespace) else {
            continue;
        };
        let Some(pid) = pid.parse().ok() else {
            continue;
        };
        let Some(info) = info(pid) else {
            continue;
        };
        all.insert(
            pid,
            Process {
                pid,
                parent: info.pbi_ppid,
                group: info.pbi_pgid,
                birth: birth(&info),
                started_at: info.pbi_start_tvsec,
                command: command.trim().into(),
            },
        );
    }

    Ok(all)
}

pub(super) fn alive(p: &Process) -> bool {
    info(p.pid).is_some_and(|info| birth(&info) == p.birth)
}

pub(super) async fn ports(processes: &[Process]) -> HashMap<u32, Vec<u16>> {
    let mut result: HashMap<u32, Vec<u16>> = HashMap::new();

    if processes.is_empty() {
        return result;
    }

    let pids = processes
        .iter()
        .map(|p| p.pid.to_string())
        .collect::<Vec<_>>()
        .join(",");
    crate::run::process::counted("lsof".as_ref());
    let output = timeout(
        Duration::from_millis(750),
        Command::new("lsof")
            .args(["-nP", "-a", "-p", &pids, "-iTCP", "-sTCP:LISTEN", "-Fpn"])
            .kill_on_drop(true)
            .output(),
    )
    .await;
    let Ok(Ok(output)) = output else {
        return result;
    };
    let mut pid = None;

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(p) = line.strip_prefix('p') {
            pid = p.parse::<u32>().ok();
        }

        if let Some((pid, port)) = pid.zip(
            line.strip_prefix('n')
                .and_then(|s| s.rsplit_once(':'))
                .and_then(|(_, s)| s.parse::<u16>().ok()),
        ) {
            result.entry(pid).or_default().push(port);
        }
    }

    for ports in result.values_mut() {
        ports.sort_unstable();
        ports.dedup();
    }

    result
}
