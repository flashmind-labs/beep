//! Whether a folder is one the machine's Syncthing keeps in step. Beep's
//! own project sync leaves such a folder alone: two tools syncing one
//! folder would fight over every change.
//!
//! Syncthing is asked through its REST API, with the GUI address and API key
//! read from its `config.xml`. When it is not installed, not running or does
//! not answer, a `.stfolder` marker in the folder or above it, up to home,
//! says the same. Nothing here fails its caller: what cannot be read counts
//! as not managed.
//!
//! A folder paused in Syncthing is not kept in step, so Beep syncs it until
//! Syncthing resumes it.

use anyhow::{Context, Result, bail};
use beep_proto::node::SyncthingFolder;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    task::spawn_blocking,
    time::timeout,
};

/// The longest a call to Syncthing may take.
const TIMEOUT: Duration = Duration::from_secs(2);
/// How long an answer about one folder stands.
const FRESH: Duration = Duration::from_secs(60);
/// Folders whose answers are kept at once.
const MAX_CACHED: usize = 256;
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

/// Where the GUI listens and the key it takes, from `config.xml`.
#[derive(Debug)]
struct Gui {
    /// Such as `127.0.0.1:8384`.
    addr: String,
    key: SecretString,
}

struct Cached {
    at: Instant,
    managed: Option<Managed>,
}

struct Inner {
    /// Folders that may hold `config.xml`; the first that does is read.
    homes: Vec<PathBuf>,
    /// The user's home: what `~` means, and where the marker search stops.
    home: Option<PathBuf>,
    cache: Mutex<HashMap<PathBuf, Cached>>,
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
            inner: Arc::new(Inner {
                homes,
                home,
                cache: Mutex::default(),
            }),
        }
    }

    /// The Syncthing folder that keeps `folder` in step, if any, as last
    /// found within a minute.
    pub async fn managed(&self, folder: &Path) -> Option<Managed> {
        if let Some(cached) = self.cache().get(folder)
            && cached.at.elapsed() < FRESH
        {
            return cached.managed.clone();
        }
        self.check(folder).await
    }

    /// The same as `managed`, asked again now.
    pub async fn check(&self, folder: &Path) -> Option<Managed> {
        let listed = self.api_folders().await;
        let managed = self.decide(folder, listed).await;
        let mut cache = self.cache();
        if cache.len() >= MAX_CACHED {
            cache.retain(|_, cached| cached.at.elapsed() < FRESH);
            if cache.len() >= MAX_CACHED {
                cache.clear();
            }
        }
        cache.insert(
            folder.to_owned(),
            Cached {
                at: Instant::now(),
                managed: managed.clone(),
            },
        );
        managed
    }

    /// Every folder Syncthing keeps in step, as its API lists them less the
    /// paused ones; empty when it does not answer.
    pub async fn folders(&self) -> Vec<SyncthingFolder> {
        self.api_folders().await.unwrap_or_default()
    }

    fn cache(&self) -> MutexGuard<'_, HashMap<PathBuf, Cached>> {
        self.inner
            .cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
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
        let homes = self.inner.homes.clone();
        let gui = match spawn_blocking(move || find_gui(&homes)).await {
            Ok(Ok(gui)) => gui?,
            Ok(Err(e)) => {
                log::debug!("Syncthing's config is unreadable: {e:#}");
                return None;
            }
            Err(e) => {
                log::warn!("reading Syncthing's config did not finish: {e}");
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

/// `GET /rest/config/folders`. HTTP/1.0, so the reply is neither chunked
/// nor kept open: it ends when the connection closes.
async fn fetch(gui: &Gui) -> Result<Vec<RawFolder>> {
    let mut stream = TcpStream::connect(&gui.addr)
        .await
        .context("reaching Syncthing")?;
    let request = format!(
        "GET /rest/config/folders HTTP/1.0\r\nHost: {}\r\nX-API-Key: {}\r\nAccept: application/json\r\n\r\n",
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
        bail!("Syncthing's folder list is over {MAX_REPLY} bytes");
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
    serde_json::from_slice(&reply[split + 4..]).context("Syncthing's folder list does not read")
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
        if let Ok(resolved) = fs::canonicalize(there) {
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
