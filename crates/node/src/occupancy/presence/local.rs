//! Local session-presence discovery. A CLI finds the node through its
//! locator: the user's, in the default node folder, for an embedded desktop
//! node and a standalone node in that folder; a node run with another
//! `--dir` keeps its own there, so it never replaces the installed node's.

use beep_proto::SessionId;
use super::Registration;
use crate::occupancy::Occupancy;
use crate::run::local_socket::{Listener, Stream};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::{
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    task::JoinSet,
    time::timeout,
};
use uuid::Uuid;

const MAX_LINE: u64 = 16 * 1024;
const HELLO_WAIT: Duration = Duration::from_secs(5);
const MAX_CONNECTIONS: usize = 256;

#[derive(Serialize)]
struct Locator<'a> {
    version: u32,
    transport: &'static str,
    address: &'a str,
    instance: Uuid,
}

struct Published {
    path: PathBuf,
    contents: String,
}

impl Drop for Published {
    fn drop(&mut self) {
        // A newer node may have advertised a different endpoint.
        if fs::read_to_string(&self.path).ok().as_ref() == Some(&self.contents)
            && let Err(e) = fs::remove_file(&self.path)
        {
            log::debug!("could not remove session presence locator: {e}");
        }
    }
}

/// Where the locator goes: `set` by `BEEP_PRESENCE_FILE`, else in the
/// standalone node's folder `dir`, else in the default node folder.
fn locator_path(set: Option<OsString>, dir: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = set {
        let path = PathBuf::from(path);

        if !path.is_absolute() {
            bail!("BEEP_PRESENCE_FILE must be absolute");
        }

        return Ok(path);
    }

    let dir = match dir {
        Some(dir) => std::path::absolute(dir)?,
        None => crate::fs::note::default_dir()?,
    };
    Ok(dir.join("presence.json"))
}

fn publish(path: &Path, address: &str) -> Result<Published> {
    let transport = if cfg!(windows) { "pipe" } else { "unix" };
    let contents = serde_json::to_string(&Locator {
        version: 1,
        transport,
        address,
        instance: Uuid::new_v4(),
    })?;
    let parent = path
        .parent()
        .context("the presence locator needs a parent folder")?;
    fs::create_dir_all(parent)?;
    let partial = parent.join(format!(".presence-{}.json", Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let written = (|| -> Result<()> {
        let mut file = options.open(&partial)?;
        file.write_all(contents.as_bytes())?;
        fs::rename(&partial, path)?;
        Ok(())
    })();

    if written.is_err() {
        let _ = fs::remove_file(&partial);
    }

    written?;
    Ok(Published {
        path: path.to_owned(),
        contents,
    })
}

/// Serves native session presence until cancelled by the node's owner,
/// with the user's locator: for a node embedded in the desktop.
pub(crate) async fn listen(occupancy: Occupancy, dir: &Path) -> Result<()> {
    let path = locator_path(std::env::var_os("BEEP_PRESENCE_FILE"), None)?;
    serve_at(occupancy, dir, path).await
}

/// As `listen`, for a standalone node in `dir`, with its locator in `dir`.
pub(crate) async fn listen_in(occupancy: Occupancy, dir: &Path) -> Result<()> {
    let path = locator_path(std::env::var_os("BEEP_PRESENCE_FILE"), Some(dir))?;
    serve_at(occupancy, dir, path).await
}

async fn serve_at(occupancy: Occupancy, dir: &Path, path: PathBuf) -> Result<()> {
    // A short private socket folder, including for long desktop paths.
    let dir = std::path::absolute(dir)?;
    let socket = crate::socket::presence_path(&dir);
    crate::socket::prepare(&socket, &dir)?;
    let mut listener = Listener::bind(&socket)?;
    #[cfg(unix)]
    let address = socket
        .to_str()
        .context("presence socket path is not UTF-8")?
        .to_owned();
    #[cfg(windows)]
    let address = crate::run::local_socket::pipe_name(&socket);
    let _published = tokio::task::spawn_blocking(move || publish(&path, &address)).await??;
    serve(&mut listener, occupancy).await
}

async fn serve(listener: &mut Listener, occupancy: Occupancy) -> Result<()> {
    let mut connections = JoinSet::new();

    loop {
        tokio::select! {
            accepted = listener.accept(), if connections.len() < MAX_CONNECTIONS => {
                let stream = accepted?;
                let occupancy = occupancy.clone();
                connections.spawn(async move {
                    if let Err(e) = connection(stream, &occupancy).await {
                        log::debug!("session presence connection ended: {e:#}");
                    }
                });
            }
            Some(result) = connections.join_next(), if !connections.is_empty() => {
                if let Err(e) = result {
                    log::debug!("session presence task ended: {e}");
                }
            }
        }
    }
}

struct Open<'a> {
    occupancy: &'a Occupancy,
    id: SessionId,
    connection: Uuid,
}

impl Drop for Open<'_> {
    fn drop(&mut self) {
        self.occupancy.presence().close(&self.id, self.connection);
    }
}

async fn read(reader: &mut BufReader<Stream>) -> Result<Option<Registration>> {
    let mut line = String::new();

    if (&mut *reader).take(MAX_LINE).read_line(&mut line).await? == 0 {
        return Ok(None);
    }

    if !line.ends_with('\n') {
        bail!("session presence line exceeds the limit or is incomplete");
    }

    Ok(Some(serde_json::from_str(&line)?))
}

async fn connection(stream: Stream, occupancy: &Occupancy) -> Result<()> {
    let mut reader = BufReader::new(stream);
    let Some(registration) = timeout(HELLO_WAIT, read(&mut reader)).await?? else {
        return Ok(());
    };
    let open = Open {
        occupancy,
        id: registration.session_id.clone(),
        connection: Uuid::new_v4(),
    };
    occupancy.presence().register(registration, open.connection)?;
    reader
        .get_mut()
        .write_all(b"{\"version\":1,\"ok\":true}\n")
        .await?;
    while let Some(registration) = read(&mut reader).await? {
        if registration.session_id != open.id {
            bail!("a presence connection belongs to one session");
        }

        occupancy.presence().register(registration, open.connection)?;
    }

    Ok(())
}

#[cfg(test)]
#[path = "../../tests/occupancy_presence_local.rs"]
mod tests;
