//! Where the machine's Syncthing listens and which folders it keeps in step,
//! as its `config.xml` and its REST API say (`Daemon`).

use crate::fs::paths;
use anyhow::{Context, Result, bail};
use beep_proto::node::SyncthingFolder;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    task::spawn_blocking,
    time::timeout,
};

/// The longest a call to Syncthing may take.
pub(crate) const TIMEOUT: Duration = Duration::from_secs(2);
/// The longest between looks at Syncthing's folders when no event says to.
pub(crate) const CHANGE_WAIT: Duration = Duration::from_secs(60);
/// The events that can change which folders Syncthing keeps in step. They
/// arrive within milliseconds of the change.
const EVENTS: &str = "FolderPaused,FolderResumed,ConfigSaved";
/// The most bytes read from the folder list.
const MAX_REPLY: usize = 8 << 20;
pub(crate) const CONFIG: &str = "config.xml";

/// Where the GUI listens and the key it takes, from `config.xml`.
#[derive(Debug)]
pub(crate) struct Gui {
    /// Such as `127.0.0.1:8384`.
    addr: String,
    key: SecretString,
}

pub struct Daemon {
    /// Folders that may hold `config.xml`; the first that does is read.
    homes: Vec<PathBuf>,
    /// The user's home: what `~` means, and where the marker search stops.
    home: Option<PathBuf>,
}


impl Daemon {
    /// The Syncthing of the user running the node, in its usual places.
    pub fn local() -> Self {
        Self::at(default_homes(), dirs::home_dir())
    }

    /// A Syncthing whose `config.xml` is in the first of `homes` that has
    /// one, for a user whose home is `home`.
    pub fn at(homes: Vec<PathBuf>, home: Option<PathBuf>) -> Self {
        Self { homes, home }
    }

    /// The user's home, where the search for markers stops.
    pub fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    /// The last event after `since`, once there is one, or `None` when
    /// `CHANGE_WAIT` passes without any.
    pub async fn next_event(&self, since: u64) -> Result<Option<u64>> {
        let gui = self.gui().await?;
        let wait = CHANGE_WAIT.as_secs();
        let path = format!("/rest/events?events={EVENTS}&since={since}&timeout={wait}");
        let events = timeout(CHANGE_WAIT + TIMEOUT, get(&gui, &path))
            .await
            .context("waiting for Syncthing's events")??;
        let events: Vec<RawEvent> =
            serde_json::from_slice(&events).context("Syncthing's events do not read")?;
        Ok(events.iter().map(|event| event.id).max())
    }


    /// The GUI to ask, from `config.xml`.
    pub(crate) async fn gui(&self) -> Result<Gui> {
        let homes = self.homes.clone();
        spawn_blocking(move || find_gui(&homes))
            .await
            .context("reading Syncthing's config did not finish")??
            .context("Syncthing has no config here")
    }


    /// The folders the API lists, or `None` when it cannot be asked.
    pub async fn folders(&self) -> Option<Vec<SyncthingFolder>> {
        let gui = match self.gui().await {
            Ok(gui) => gui,
            Err(e) => {
                log::debug!("Syncthing cannot be asked: {e:#}");
                return None;
            }
        };
        let raw = match timeout(TIMEOUT, fetch(&gui)).await {
            Ok(Ok(raw)) => raw,
            Ok(Err(e)) => {
                log::debug!("Syncthing did not list its folders: {e:#}");
                return None;
            }
            Err(e) => {
                log::debug!("Syncthing did not list its folders: {e:#}");
                return None;
            }
        };

        let home = self.home.clone();

        match spawn_blocking(move || resolve(raw, home.as_deref())).await {
            Ok(folders) => Some(folders),
            Err(e) => {
                log::warn!("resolving Syncthing's folders did not finish: {e}");
                None
            }
        }
    }
}

/// Where Syncthing keeps `config.xml` on this OS, most likely first.
fn default_homes() -> Vec<PathBuf> {
    let mut homes = Vec::new();
    #[cfg(target_os = "macos")]
    homes.extend(dirs::config_dir().map(|dir| dir.join("Syncthing")));
    #[cfg(windows)]
    homes.extend(dirs::data_local_dir().map(|dir| dir.join("Syncthing")));
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        homes.extend(dirs::state_dir().map(|dir| dir.join("syncthing")));
        homes.extend(dirs::config_dir().map(|dir| dir.join("syncthing")));
    }
    homes
}

/// The GUI of the first `config.xml` in `homes`; `None` when there is none.
fn find_gui(homes: &[PathBuf]) -> Result<Option<Gui>> {
    for home in homes {
        let file = home.join(CONFIG);

        match fs::read_to_string(&file) {
            Ok(text) => return parse_gui(&text).map(Some),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e).with_context(|| format!("reading {}", file.display())),
        }
    }

    Ok(None)
}

/// The GUI address and API key of a `config.xml`.
fn parse_gui(text: &str) -> Result<Gui> {
    let doc = roxmltree::Document::parse(text).context("parsing config.xml")?;
    let gui = doc
        .root_element()
        .children()
        .find(|node| node.has_tag_name("gui"))
        .context("config.xml has no <gui>")?;
    let child = |name: &str| {
        gui.children()
            .find(|node| node.has_tag_name(name))
            .and_then(|node| node.text())
            .map(str::trim)
            .filter(|text| !text.is_empty())
    };
    let address = child("address").context("config.xml has no GUI address")?;
    let key = child("apikey").context("config.xml has no API key")?;
    let tls = gui.attribute("tls") == Some("true");
    Ok(Gui {
        addr: gui_addr(address, tls)?,
        key: SecretString::from(key.to_owned()),
    })
}

/// The `host:port` the GUI is reached at from this machine. A GUI that
/// takes HTTPS only is not asked: the markers answer for it.
fn gui_addr(address: &str, tls: bool) -> Result<String> {
    if tls || address.starts_with("https://") {
        bail!("the GUI takes HTTPS only");
    }

    let rest = address.strip_prefix("http://").unwrap_or(address);

    if rest.starts_with("unix") || rest.starts_with('/') {
        bail!("the GUI listens on a socket");
    }

    let rest = rest.trim_end_matches('/');
    // Listening on every address is reached on loopback.
    Ok(if let Some(port) = rest.strip_prefix("0.0.0.0:") {
        format!("127.0.0.1:{port}")
    } else if let Some(port) = rest.strip_prefix("[::]:") {
        format!("[::1]:{port}")
    } else if let Some(port) = rest.strip_prefix(':') {
        format!("127.0.0.1:{port}")
    } else {
        rest.to_owned()
    })
}

/// One folder as `/rest/config/folders` lists it.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct RawFolder {
    id: String,
    label: String,
    path: String,
    pub(crate) paused: bool,
}

/// One event as `/rest/events` lists it.
#[derive(Debug, Deserialize)]
struct RawEvent {
    id: u64,
}

/// `GET /rest/config/folders`.
pub(crate) async fn fetch(gui: &Gui) -> Result<Vec<RawFolder>> {
    let body = get(gui, "/rest/config/folders").await?;
    serde_json::from_slice(&body).context("Syncthing's folder list does not read")
}

/// The body of `GET path`. HTTP/1.0, so the reply is neither chunked nor
/// kept open: it ends when the connection closes.
pub(crate) async fn get(gui: &Gui, path: &str) -> Result<Vec<u8>> {
    request(gui, "GET", path, &[]).await
}

pub(crate) async fn request(gui: &Gui, method: &str, path: &str, body: &[u8]) -> Result<Vec<u8>> {
    let mut stream = TcpStream::connect(&gui.addr)
        .await
        .context("reaching Syncthing")?;
    let request = format!(
        "{method} {path} HTTP/1.0\r\nHost: {}\r\nX-API-Key: {}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        gui.addr,
        gui.key.expose_secret(),
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .context("asking Syncthing")?;
    stream
        .write_all(body)
        .await
        .context("sending Syncthing's request body")?;
    let mut reply = Vec::new();
    let limit = u64::try_from(MAX_REPLY + 1)?;
    (&mut stream)
        .take(limit)
        .read_to_end(&mut reply)
        .await
        .context("reading Syncthing's reply")?;
    if reply.len() > MAX_REPLY {
        bail!("Syncthing's reply is over {MAX_REPLY} bytes");
    }

    let split = reply
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .context("Syncthing's reply has no body")?;
    let head = String::from_utf8_lossy(&reply[..split]);
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or_default();
    if status != "200" {
        bail!("Syncthing answered {status}");
    }

    Ok(reply.split_off(split + 4))
}

/// `value` as it goes in a URL's query.
pub(crate) fn query_value(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'-' | b'_' | b'.' | b'~' => char::from(byte).to_string(),
            _ if byte.is_ascii_alphanumeric() => char::from(byte).to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// The folders not paused, with `~` expanded and symlinks resolved.
pub(crate) fn resolve(raw: Vec<RawFolder>, home: Option<&Path>) -> Vec<SyncthingFolder> {
    raw.into_iter()
        .filter(|folder| !folder.path.is_empty() && !folder.paused)
        .map(|folder| {
            let path = real(&expand(&folder.path, home));
            SyncthingFolder {
                id: folder.id,
                label: folder.label,
                path: path.to_string_lossy().into_owned(),
            }
        })
        .collect()
}

/// `path` with symlinks resolved as far as it exists; the part not there
/// yet is kept as written.
pub(crate) fn real(path: &Path) -> PathBuf {
    for there in path.ancestors() {
        if let Ok(resolved) = paths::canonicalize(there) {
            return match path.strip_prefix(there) {
                Ok(rest) if !rest.as_os_str().is_empty() => resolved.join(rest),
                _ => resolved,
            };
        }
    }

    path.to_owned()
}

/// `path` with a leading `~` as `home`.
pub(crate) fn expand(path: &str, home: Option<&Path>) -> PathBuf {
    let rest = path
        .strip_prefix("~/")
        .or_else(|| path.strip_prefix("~\\"))
        .or_else(|| (path == "~").then_some(""));
    match (rest, home) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(path),
    }
}

#[cfg(test)]
#[path = "../../tests/machine_env_syncthing.rs"]
mod tests;
