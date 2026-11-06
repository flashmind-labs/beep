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
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::task::spawn_blocking;

/// Answers the one `Request` the stream carries.
pub(crate) async fn serve_report(
    mut tx: LinkTx,
    mut rx: LinkRx,
    status: LocalStatus,
    log: Logs,
) -> Result<()> {
    let Some(Request { since, max_bytes }) = rx.recv().await? else {
        return Ok(());
    };
    let reply = match i64::try_from(since) {
        Ok(since) => {
            let max_bytes = usize::try_from(max_bytes.min(MAX_BYTES))?;
            // A log that cannot be read still leaves the rest worth sending.
            let log = spawn_blocking(move || log.window(since, max_bytes))
                .await?
                .unwrap_or_else(|e| format!("the log could not be read: {e:#}"));
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |now| now.as_secs());
            Reply::Share {
                log,
                status: format!(
                    "{}\n{}",
                    render(&status, now),
                    programs(spawn_env::search_path(), dirs::home_dir().as_deref())
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
