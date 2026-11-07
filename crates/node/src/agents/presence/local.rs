//! Local session-presence discovery. The locator is per user so a CLI
//! finds an embedded desktop node or a standalone node with any --dir.

use super::Registration;
use crate::agents::Agents;
use crate::link::local_socket::{Listener, Stream};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
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

fn locator_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("BEEP_PRESENCE_FILE") {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            bail!("BEEP_PRESENCE_FILE must be absolute");
        }
        return Ok(path);
    }
    Ok(crate::link::state::default_dir()?.join("presence.json"))
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

/// Serves native session presence until cancelled by the node's owner.
pub async fn listen(agents: Arc<Agents>, dir: &Path) -> Result<()> {
    // A short private socket folder, including for long desktop paths.
    let dir = std::path::absolute(dir)?;
    let socket = super::super::mcp::presence_socket(&dir);
    super::super::mcp::prepare_presence_socket(&socket, &dir)?;
    let mut listener = Listener::bind(&socket)?;
    #[cfg(unix)]
    let address = socket
        .to_str()
        .context("presence socket path is not UTF-8")?
        .to_owned();
    #[cfg(windows)]
    let address = crate::link::local_socket::pipe_name(&socket);
    let path = locator_path()?;
    let _published = tokio::task::spawn_blocking(move || publish(&path, &address)).await??;
    serve(&mut listener, agents).await
}

async fn serve(listener: &mut Listener, agents: Arc<Agents>) -> Result<()> {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept(), if connections.len() < MAX_CONNECTIONS => {
                let stream = accepted?;
                let agents = agents.clone();
                connections.spawn(async move {
                    if let Err(e) = connection(stream, &agents).await {
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
    agents: &'a Agents,
    id: String,
    connection: Uuid,
}

impl Drop for Open<'_> {
    fn drop(&mut self) {
        self.agents.presence.close(&self.id, self.connection);
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

async fn connection(stream: Stream, agents: &Agents) -> Result<()> {
    let mut reader = BufReader::new(stream);
    let Some(registration) = timeout(HELLO_WAIT, read(&mut reader)).await?? else {
        return Ok(());
    };
    let open = Open {
        agents,
        id: registration.session_id.clone(),
        connection: Uuid::new_v4(),
    };
    agents.presence.register(registration, open.connection)?;
    reader
        .get_mut()
        .write_all(b"{\"version\":1,\"ok\":true}\n")
        .await?;
    while let Some(registration) = read(&mut reader).await? {
        if registration.session_id != open.id {
            bail!("a presence connection belongs to one session");
        }
        agents.presence.register(registration, open.connection)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::node::agent::Presence;

    #[test]
    fn an_old_listener_leaves_a_replacement_locator_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("presence.json");
        let old = publish(&path, "/srv/node/presence.sock").unwrap();
        let new = publish(&path, "/srv/node/presence.sock").unwrap();
        let before = fs::read(&path).unwrap();
        drop(old);
        assert_eq!(fs::read(&path).unwrap(), before);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        drop(new);
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_idle_connection_stays_present_until_eof_or_cancellation() {
        for cancel in [false, true] {
            let agents = Arc::new(Agents::default());
            let mut changes = agents.presence.subscribe();
            let (mut client, server) = tokio::net::UnixStream::pair().unwrap();
            let serving = agents.clone();
            let task = tokio::spawn(async move { connection(server, &serving).await });
            let registration = serde_json::json!({
                "version": 1, "agent": "flsh", "session_id": "session_a",
                "cwd": "/srv/project", "pid": 123
            });
            client
                .write_all(format!("{registration}\n").as_bytes())
                .await
                .unwrap();
            let mut reader = BufReader::new(client);
            let mut answer = String::new();
            timeout(HELLO_WAIT, reader.read_line(&mut answer))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&answer).unwrap()["ok"],
                true
            );
            assert_eq!(agents.presence.snapshot()[0].presence, Presence::Running);
            changes.borrow_and_update();
            if cancel {
                task.abort();
                assert!(task.await.unwrap_err().is_cancelled());
            } else {
                drop(reader);
                task.await.unwrap().unwrap();
            }
            timeout(HELLO_WAIT, changes.changed())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(agents.presence.snapshot()[0].presence, Presence::Stopped);
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn oversized_registrations_are_refused_without_tracking_a_session() {
        let agents = Arc::new(Agents::default());
        let serving = agents.clone();
        let (mut client, server) = tokio::net::UnixStream::pair().unwrap();
        let task = tokio::spawn(async move { connection(server, &serving).await });
        client
            .write_all(&vec![b'x'; MAX_LINE as usize])
            .await
            .unwrap();
        assert!(timeout(HELLO_WAIT, task).await.unwrap().unwrap().is_err());
        assert!(agents.presence.snapshot().is_empty());
    }
}
