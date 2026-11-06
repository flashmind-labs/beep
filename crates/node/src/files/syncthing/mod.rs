//! Whether the machine's Syncthing keeps a folder in step
//! (`Syncthing::check`). Beep's own sync leaves such a folder alone, since
//! two tools syncing one folder fight over every change. A folder paused in
//! Syncthing is not kept in step, so Beep syncs it until Syncthing resumes it.

use crate::files::paths;
use anyhow::{Context, Result, bail};
use beep_proto::node::SyncthingFolder;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    task::spawn_blocking,
    time::{sleep, timeout},
};

/// The longest a call to Syncthing may take.
const TIMEOUT: Duration = Duration::from_secs(2);
/// The longest between looks at Syncthing's folders when no event says to.
const CHANGE_WAIT: Duration = Duration::from_secs(60);
/// The events that can change which folders Syncthing keeps in step. They
/// arrive within milliseconds of the change.
const EVENTS: &str = "FolderPaused,FolderResumed,ConfigSaved";
/// The most bytes read from the folder list.
const MAX_REPLY: usize = 8 << 20;
/// What Syncthing leaves in the root of each folder it syncs.
const MARKER: &str = ".stfolder";
const CONFIG: &str = "config.xml";

/// The Syncthing folder that keeps a folder in step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Managed {
    /// Its folder ID, when the API named it.
    pub id: Option<String>,
    /// What to call it: its label or ID, or the marked folder's name.
    pub label: String,
    /// The Syncthing folder itself, the one given or one above it.
    pub root: PathBuf,
}

impl Managed {
    /// Why Beep does not sync the folder.
    pub fn reason(&self) -> String {
        format!("Already synced by Syncthing ({})", self.label)
    }
}

/// How a folder stands, as `/rest/db/status` says.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FolderStatus {
    pub(crate) need_total_items: u64,
    state: String,
}

impl FolderStatus {
    /// Syncthing has everything it knows of and is not working on the folder.
    pub(crate) fn is_complete(&self) -> bool {
        self.need_total_items == 0 && self.state == "idle"
    }
}

/// Where the GUI listens and the key it takes, from `config.xml`.
#[derive(Debug)]
struct Gui {
    /// Such as `127.0.0.1:8384`.
    addr: String,
    key: SecretString,
}

struct Inner {
    /// Folders that may hold `config.xml`; the first that does is read.
    homes: Vec<PathBuf>,
    /// The user's home: what `~` means, and where the marker search stops.
    home: Option<PathBuf>,
}

/// The machine's Syncthing, as far as it can be found.
#[derive(Clone)]
pub struct Syncthing {
    inner: Arc<Inner>,
}

impl Syncthing {
    /// The Syncthing of the user running the node, in its usual places.
    pub fn local() -> Self {
        Self::at(default_homes(), dirs::home_dir())
    }

    /// A Syncthing whose `config.xml` is in the first of `homes` that has
    /// one, for a user whose home is `home`.
    pub fn at(homes: Vec<PathBuf>, home: Option<PathBuf>) -> Self {
        Self {
            inner: Arc::new(Inner { homes, home }),
        }
    }

    /// The Syncthing folder that keeps `folder` in step, if any. Asks
    /// Syncthing's REST API, with the address and key from its `config.xml`; when it does not
    /// answer, a `.stfolder` marker in the folder or above it, up to home,
    /// counts. Never fails: what cannot be read counts as not managed.
    pub async fn check(&self, folder: &Path) -> Option<Managed> {
        let listed = self.api_folders().await;
        self.decide(folder, listed).await
    }

    /// How the Syncthing folder `id` stands now.
    pub(crate) async fn folder_status(&self, id: &str) -> Result<FolderStatus> {
        let gui = self.gui().await?;
        let path = format!("/rest/db/status?folder={}", query_value(id));
        let body = timeout(TIMEOUT, get(&gui, &path))
            .await
            .context("waiting for Syncthing's folder status")??;
        serde_json::from_slice(&body).context("Syncthing's folder status does not read")
    }

    /// Waits until Syncthing pauses, resumes or reconfigures a folder, or
    /// `CHANGE_WAIT` has passed. `since` is the last event seen, 0 at first.
    /// When its events cannot be read, waits out `CHANGE_WAIT`.
    pub(crate) async fn changed(&self, since: &mut u64) {
        match self.next_event(*since).await {
            Ok(last) => *since = last.unwrap_or(*since),
            Err(e) => {
                log::debug!("Syncthing's events did not read: {e:#}");
                // Its event numbers start over when it restarts.
                *since = 0;
                sleep(CHANGE_WAIT).await;
            }
        }
    }

    /// The last event after `since`, once there is one, or `None` when
    /// `CHANGE_WAIT` passes without any.
    async fn next_event(&self, since: u64) -> Result<Option<u64>> {
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
    async fn gui(&self) -> Result<Gui> {
        let homes = self.inner.homes.clone();
        spawn_blocking(move || find_gui(&homes))
            .await
            .context("reading Syncthing's config did not finish")??
            .context("Syncthing has no config here")
    }

    /// Every folder Syncthing keeps in step, as its API lists them less the
    /// paused ones; empty when it does not answer.
    pub async fn folders(&self) -> Vec<SyncthingFolder> {
        self.api_folders().await.unwrap_or_default()
    }

    async fn decide(&self, folder: &Path, listed: Option<Vec<SyncthingFolder>>) -> Option<Managed> {
        let folder = folder.to_owned();
        let home = self.inner.home.clone();
        let decided = spawn_blocking(move || {
            let folder = real(&folder);
            let home = home.as_deref().map(real);
            match listed {
                Some(listed) => holding(&listed, &folder),
                None => marked(&folder, home.as_deref()),
            }
        })
        .await;
        match decided {
            Ok(managed) => managed,
            Err(e) => {
                log::warn!("the Syncthing check did not finish: {e}");
                None
            }
        }
    }

    /// The folders the API lists, or `None` when it cannot be asked.
    async fn api_folders(&self) -> Option<Vec<SyncthingFolder>> {
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
        let home = self.inner.home.clone();
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
struct RawFolder {
    id: String,
    label: String,
    path: String,
    paused: bool,
}

/// One event as `/rest/events` lists it.
#[derive(Debug, Deserialize)]
struct RawEvent {
    id: u64,
}

/// `GET /rest/config/folders`.
async fn fetch(gui: &Gui) -> Result<Vec<RawFolder>> {
    let body = get(gui, "/rest/config/folders").await?;
    serde_json::from_slice(&body).context("Syncthing's folder list does not read")
}

/// The body of `GET path`. HTTP/1.0, so the reply is neither chunked nor
/// kept open: it ends when the connection closes.
async fn get(gui: &Gui, path: &str) -> Result<Vec<u8>> {
    let mut stream = TcpStream::connect(&gui.addr)
        .await
        .context("reaching Syncthing")?;
    let request = format!(
        "GET {path} HTTP/1.0\r\nHost: {}\r\nX-API-Key: {}\r\nAccept: application/json\r\n\r\n",
        gui.addr,
        gui.key.expose_secret()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .context("asking Syncthing")?;
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
fn query_value(value: &str) -> String {
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
fn resolve(raw: Vec<RawFolder>, home: Option<&Path>) -> Vec<SyncthingFolder> {
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
fn real(path: &Path) -> PathBuf {
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
fn expand(path: &str, home: Option<&Path>) -> PathBuf {
    let rest = path
        .strip_prefix("~/")
        .or_else(|| path.strip_prefix("~\\"))
        .or_else(|| (path == "~").then_some(""));
    match (rest, home) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(path),
    }
}

/// The innermost of `listed` that is `folder` or holds it.
fn holding(listed: &[SyncthingFolder], folder: &Path) -> Option<Managed> {
    listed
        .iter()
        .filter(|synced| folder.starts_with(Path::new(&synced.path)))
        .max_by_key(|synced| Path::new(&synced.path).components().count())
        .map(|synced| Managed {
            id: Some(synced.id.clone()),
            label: synced.name().to_owned(),
            root: PathBuf::from(&synced.path),
        })
}

/// The folder at or above `folder`, up to `home`, that holds a marker.
fn marked(folder: &Path, home: Option<&Path>) -> Option<Managed> {
    for dir in folder.ancestors() {
        if dir.join(MARKER).exists() {
            let label = dir
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| dir.to_string_lossy().into_owned());
            return Some(Managed {
                id: None,
                label,
                root: dir.to_owned(),
            });
        }
        if home == Some(dir) {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests;
