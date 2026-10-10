//! Admin commands over a Unix socket, so only something on the server itself
//! can send them (`beep-admin`). One command per connection: a line in, the
//! reply lines out.
//!
//! `rescan [USER_ID]` reads the connected mailboxes and calendars of every
//! user, or of one, again from the start and turns them into memories.

use super::App;
use crate::profile;
use anyhow::{Context, Result};
use std::{
    fs,
    io::ErrorKind,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};

/// Listens on `path`, readable and writable by the server's user only.
pub fn serve(app: Arc<App>, path: PathBuf) -> Result<()> {
    match fs::remove_file(&path) {
        Err(error) if error.kind() != ErrorKind::NotFound => {
            return Err(error).with_context(|| format!("removing {}", path.display()));
        }
        _ => {}
    }
    let listener =
        UnixListener::bind(&path).with_context(|| format!("binding {}", path.display()))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    log::info!("admin socket at {}", path.display());
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let app = app.clone();
                    tokio::spawn(async move {
                        if let Err(error) = answer(&app, stream).await {
                            log::warn!("admin: {error:#}");
                        }
                    });
                }
                Err(error) => log::warn!("admin: accept failed: {error}"),
            }
        }
    });
    Ok(())
}

/// The socket next to the config file.
pub fn default_path(config: &Path) -> PathBuf {
    config.with_file_name("admin.sock")
}

async fn answer(app: &Arc<App>, stream: UnixStream) -> Result<()> {
    let (read, mut write) = stream.into_split();
    let mut line = String::new();
    BufReader::new(read).read_line(&mut line).await?;
    let words: Vec<&str> = line.split_whitespace().collect();
    let reply = match words.as_slice() {
        ["rescan"] => rescan(app, None).await,
        ["rescan", user] => rescan(app, Some(user)).await,
        _ => Ok(vec!["usage: rescan [USER_ID]".to_owned()]),
    };
    let lines = reply.unwrap_or_else(|error| vec![format!("error: {error:#}")]);
    for line in lines {
        write.write_all(line.as_bytes()).await?;
        write.write_all(b"\n").await?;
    }
    Ok(())
}

async fn rescan(app: &Arc<App>, only: Option<&str>) -> Result<Vec<String>> {
    let users: Vec<_> = app
        .users()
        .into_iter()
        .filter(|user| only.is_none_or(|id| user.id() == id))
        .collect();
    if users.is_empty() {
        return Ok(vec!["no such user".to_owned()]);
    }
    let mut lines = Vec::new();
    let mut rescanned = 0;
    for user in &users {
        let connected: Vec<String> = match user.apps().await {
            Ok(apps) => apps
                .into_iter()
                .filter(|app| app.connected)
                .map(|app| app.slug)
                .collect(),
            Err(error) => {
                lines.push(format!("{}: apps not listed: {error:?}", user.id()));
                continue;
            }
        };
        let started = match profile::reread(app, user, &connected).await {
            Ok(started) => started,
            Err(error) => {
                lines.push(format!("{}: rescan not started: {error:?}", user.id()));
                continue;
            }
        };
        if started.is_empty() {
            continue;
        }
        let names: Vec<&str> = started.iter().map(|source| source.key()).collect();
        lines.push(format!("{}: {}", user.id(), names.join(", ")));
        rescanned += 1;
    }
    log::info!("admin: rescan started; users={rescanned}");
    lines.push(format!("rescan started for {rescanned} users"));
    Ok(lines)
}
