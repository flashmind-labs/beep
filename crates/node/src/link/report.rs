//! `report/1` (see `beep_proto::node::report`): this machine's part of a
//! problem report, its log for a period, status, version and OS.

use crate::{
    agents::locate,
    link::local_status::{LocalStatus, render},
    machine::{env::os_version, provision::spawn_env, service::logs::Logs},
};
use anyhow::Result;
use beep_proto::node::{
    LinkRx, LinkTx,
    report::{MAX_BYTES, Reply, Request},
};
use std::{
    env::consts::OS,
    ffi::OsString,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::task::spawn_blocking;

/// Answers the one `Request` the stream carries.
pub(crate) async fn serve_report(
    mut tx: LinkTx,
    mut rx: LinkRx,
    status: LocalStatus,
    log: Logs,
    metrics: PathBuf,
) -> Result<()> {
    let Some(Request { since, max_bytes }) = rx.recv().await? else {
        return Ok(());
    };
    let reply = match i64::try_from(since) {
        Ok(since) => {
            let max_bytes = usize::try_from(max_bytes.min(MAX_BYTES))?;
            // A log that cannot be read still leaves the rest worth sending.
            let (log, metrics) = spawn_blocking(move || {
                (
                    log.window(since, max_bytes),
                    recent_metrics(&metrics, since, max_bytes),
                )
            })
            .await?;
            let log = log.unwrap_or_else(|e| format!("the log could not be read: {e:#}"));
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |now| now.as_secs());
            Reply::Share {
                log,
                status: format!(
                    "{}\n{}\n{}",
                    render(&status, now),
                    programs(spawn_env::search_path(), dirs::home_dir().as_deref()),
                    metrics
                ),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                os: OS.to_owned(),
                os_version: os_version().await,
            }
        }
        Err(_) => Reply::Failed {
            reason: format!("{since} is not a time this machine can read its log from"),
        },
    };
    tx.send(&reply).await?;
    tx.finish().await
}

/// Includes recent complete snapshots without reading an entire day's file.
fn recent_metrics(path: &Path, since: i64, max_bytes: usize) -> String {
    const LIMIT: usize = 256 * 1024;
    let limit = max_bytes.min(LIMIT);
    if limit == 0 {
        return String::new();
    }
    let mut paths: Vec<_> = path
        .parent()
        .and_then(|dir| std::fs::read_dir(dir).ok())
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("metrics.") && name.ends_with(".log") && name != "metrics.log"
                })
        })
        .collect();
    paths.sort();
    paths.push(path.to_owned());
    let mut chunks = Vec::new();
    let mut remaining = limit;
    for path in paths.into_iter().rev() {
        if remaining == 0 {
            break;
        }
        let Ok(mut file) = std::fs::File::open(path) else {
            continue;
        };
        let Ok(size) = file.metadata().map(|m| m.len()) else {
            continue;
        };
        let start = size.saturating_sub(remaining as u64);
        if file.seek(SeekFrom::Start(start)).is_err() {
            continue;
        }
        let mut bytes = Vec::new();
        if file.take(size - start).read_to_end(&mut bytes).is_err() {
            continue;
        }
        remaining = remaining.saturating_sub(bytes.len());
        chunks.push(bytes);
    }
    let mut result = String::new();
    for bytes in chunks.into_iter().rev() {
        let text = String::from_utf8_lossy(&bytes);
        for snapshot in text.split("# snapshot unix_seconds=").skip(1) {
            let Some((time, _)) = snapshot.split_once('\n') else {
                continue;
            };
            if snapshot.contains("# end snapshot\n")
                && time.parse::<i64>().is_ok_and(|time| time >= since)
            {
                result.push_str("# snapshot unix_seconds=");
                result.push_str(snapshot);
            }
        }
    }
    if result.is_empty() {
        result
    } else {
        format!("Metrics\n{result}")
    }
}

/// The programs the agents need, read by hand when an agent goes missing.
const PROGRAMS: &[&str] = &[
    "claude",
    "codex",
    "gemini",
    "pi",
    "flsh",
    "opencode",
    "claude-agent-acp",
    "codex-acp",
    "pi-acp",
    "node",
    "npx",
];

/// Where each of `PROGRAMS` is: on `path`, in an install folder under
/// `home`, or nowhere, then `path` itself.
fn programs(path: Option<OsString>, home: Option<&Path>) -> String {
    let mut text = String::from("Programs\n");
    for name in PROGRAMS {
        let found = match which::which_in(name, path.as_ref(), "/") {
            Ok(at) => format!("{} (on PATH)", at.display()),
            Err(_) => match home.and_then(|home| locate::fallback(name, home)) {
                Some(at) => format!("{} (not on PATH)", at.display()),
                None => "not found".to_owned(),
            },
        };
        text.push_str(&format!("  {name:<17} {found}\n"));
    }
    let path = path.unwrap_or_default();
    text.push_str(&format!("PATH\n  {}\n", path.to_string_lossy()));
    text
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};

    #[test]
    fn metrics_report_keeps_only_snapshots_in_the_requested_period() {
        let dir = tempfile::tempdir().expect("directory");
        let path = dir.path().join("metrics.log");
        fs::write(
            &path,
            "# snapshot unix_seconds=10\na 1\n# end snapshot\n# snapshot unix_seconds=20\na 2\n# end snapshot\n# snapshot unix_seconds=30\na 3\n",
        )
        .expect("metrics");
        let text = recent_metrics(&path, 15, 1024);
        assert!(!text.contains("a 1"));
        assert!(text.contains("# snapshot unix_seconds=20\na 2"));
        assert!(!text.contains("a 3"));
        assert!(recent_metrics(&path, 0, 0).is_empty());
        fs::write(
            dir.path().join("metrics.2026-01-01.log"),
            "# snapshot unix_seconds=5\na 0\n# end snapshot\n",
        )
        .expect("rotated metrics");
        assert!(recent_metrics(&path, 0, 1024).contains("a 0"));
    }

    #[test]
    fn a_program_outside_path_is_listed_with_its_folder() {
        let home = tempfile::tempdir().expect("a temp folder");
        let bin = home.path().join(".local/bin");
        fs::create_dir_all(&bin).expect("folders");
        let claude = bin.join("claude");
        fs::write(&claude, "#!/bin/sh\n").expect("a program");
        fs::set_permissions(&claude, fs::Permissions::from_mode(0o755)).expect("executable");
        let empty = tempfile::tempdir().expect("a temp folder");
        let text = programs(Some(empty.path().into()), Some(home.path()));
        let line = text
            .lines()
            .find(|line| line.trim_start().starts_with("claude "))
            .expect("a claude line");
        assert!(
            line.ends_with(&format!("{} (not on PATH)", claude.display())),
            "{line}"
        );
        assert!(text.contains("codex-acp"), "{text}");
    }
}
