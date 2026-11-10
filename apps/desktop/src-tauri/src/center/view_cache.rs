//! The views the window last drew, in one file, so a new window shows them
//! at once. Only what the mesh or the server lists goes in, never a key. A
//! new app version keeps only the typed local search lists.

use crate::{
    beep::{Service, state::LinkState},
    error::{self, DesktopError},
};
use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tauri::State;

/// Raised whenever the file's own layout changes.
const FORMAT: u32 = 1;
/// How long a changed view waits for the file, so a burst of changes, such
/// as a streaming session's, writes it once.
const WRITE_AFTER: Duration = Duration::from_secs(2);

/// One view the window draws from the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum View {
    /// `center_home`.
    Home,
    /// `projects_on_top`.
    Top,
    /// `project_groups`.
    Groups,
    /// `project_sync`.
    Sync,
    /// `center_projects`.
    Agents,
    /// `project_overview`.
    Overview,
    /// The machines' agent sessions as the server listed them, which Home
    /// uses while the machines are slow to answer.
    AgentSessions,
    /// What each project's page last read, section by section, as the page
    /// saves it (`pagecache.ts`), with the account it belongs to and its
    /// size capped there.
    Pages,
    /// The lists Cmd+K searches, which this side reads and ranks from.
    Palette,
    /// What Home's pull requests and issues were last made of, which `git_items` starts from:
    /// no token, and a key that is a hash of the tokens and projects they were read with.
    GitItems,
}

#[derive(Serialize, Deserialize)]
struct CacheFile {
    format: u32,
    /// The app version that wrote it. The page reads the views in that
    /// version's shape, so another version starts over.
    app: String,
    views: BTreeMap<View, Value>,
}

pub struct ViewCache {
    path: PathBuf,
    app: String,
    views: Mutex<BTreeMap<View, Value>>,
    /// Changes not yet written. A writer runs while it is above zero.
    unsaved: AtomicUsize,
    /// Held while the file is written, so an older write never lands last.
    saving: Mutex<()>,
}

impl ViewCache {
    /// Reads the file at `path`, written by app version `app`. A missing,
    /// unreadable or outdated one starts empty.
    pub fn load(path: PathBuf, app: &str) -> Self {
        let views = match read(&path, app) {
            Ok(views) => views,
            Err(e) => {
                log::info!("starting without the saved views: {e:#}");
                BTreeMap::new()
            }
        };
        Self {
            path,
            app: app.to_owned(),
            views: Mutex::new(views),
            unsaved: AtomicUsize::new(0),
            saving: Mutex::new(()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, BTreeMap<View, Value>> {
        self.views.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Every view held.
    pub fn views(&self) -> BTreeMap<View, Value> {
        self.lock().clone()
    }

    pub fn get(&self, view: View) -> Option<Value> {
        self.lock().get(&view).cloned()
    }

    /// Takes `value` as the latest `view`. True when the caller must run
    /// `write_unsaved` on a blocking thread; false when nothing changed, or
    /// when a writer runs already and will write this too.
    pub fn put(&self, view: View, value: Value) -> bool {
        {
            let mut views = self.lock();

            if views.get(&view) == Some(&value) {
                return false;
            }

            views.insert(view, value);
        }
        self.unsaved.fetch_add(1, Ordering::AcqRel) == 0
    }

    /// Writes the file until every change is in it. It blocks, so it runs
    /// off the async workers, and only one runs at a time.
    pub fn write_unsaved(&self) {
        loop {
            let seen = self.unsaved.load(Ordering::Acquire);

            if let Err(e) = self.save() {
                log::warn!("saving the views: {e:#}");
            }
            // A change made while writing keeps the count up: write again.
            if self.unsaved.fetch_sub(seen, Ordering::AcqRel) == seen {
                return;
            }
        }
    }

    /// Writes the changes a writer still waits to write, when the app quits.
    pub fn flush(&self) {
        if self.unsaved.load(Ordering::Acquire) == 0 {
            return;
        }

        if let Err(e) = self.save() {
            log::warn!("saving the views: {e:#}");
        }
    }

    /// Writes what is held now, through a partial file so a reader never
    /// sees half of it.
    fn save(&self) -> anyhow::Result<()> {
        let _saving = self.saving.lock().unwrap_or_else(PoisonError::into_inner);
        let bytes = {
            let views = self.lock();
            serde_json::to_vec(&CacheFileRef {
                format: FORMAT,
                app: &self.app,
                views: &views,
            })?
        };

        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("making {}", dir.display()))?;
        }

        let mut partial = self.path.as_os_str().to_owned();
        partial.push(".part");
        fs::write(&partial, &bytes).with_context(|| format!("writing {}", self.path.display()))?;
        fs::rename(&partial, &self.path)
            .with_context(|| format!("replacing {}", self.path.display()))?;
        metrics::counter!("beep_persist_writes_total", "file" => "view_cache").increment(1);
        Ok(())
    }

    /// Drops what only the server lists, when server views are hidden: the project agents, the
    /// overview and Home's server sections. What the mesh gives stays, so a window draws Home,
    /// the sidebar and project pages from disk at once; the first fresh read replaces each.
    /// Writes the file only when something went.
    pub fn keep_local(&self) {
        let changed = {
            let mut views = self.lock();
            let agents = views.remove(&View::Agents).is_some();
            let overview = views.remove(&View::Overview).is_some();
            let home = views.get_mut(&View::Home).is_some_and(local_home);
            agents || overview || home
        };

        if changed && let Err(e) = self.save() {
            log::warn!("saving the local views: {e:#}");
        }
    }

    /// Forgets every view, when this computer starts over.
    pub fn clear(&self) {
        self.lock().clear();

        match fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => log::warn!("removing {}: {e}", self.path.display()),
        }
    }
}

/// Home without what only the server lists: researches, chat approvals, routines, app updates
/// and machines the mesh does not hold. True when any went.
fn local_home(home: &mut Value) -> bool {
    let Some(home) = home.as_object_mut() else {
        return false;
    };
    let before = home.clone();
    let kind_is = |row: &Value, kind: &str| row.get("kind").and_then(Value::as_str) == Some(kind);

    if let Some(Value::Array(running)) = home.get_mut("running") {
        running.retain(|row| !kind_is(row, "research"));
    }

    if let Some(Value::Array(needs)) = home.get_mut("needs_you") {
        needs.retain(|row| !kind_is(row, "chat"));
    }

    if let Some(Value::Array(machines)) = home.get_mut("machines") {
        machines.retain(|card| card.get("joined").and_then(Value::as_bool) != Some(false));
    }

    for section in ["coming_up", "updates"] {
        if let Some(value) = home.get_mut(section) {
            *value = Value::Null;
        }
    }

    *home != before
}

/// `CacheFile` borrowed, so saving does not copy the views.
#[derive(Serialize)]
struct CacheFileRef<'a> {
    format: u32,
    app: &'a str,
    views: &'a BTreeMap<View, Value>,
}

fn read(path: &Path, app: &str) -> anyhow::Result<BTreeMap<View, Value>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let file: CacheFile =
        serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))?;
    if file.format != FORMAT {
        bail!(
            "{} is from format {} of app {}",
            path.display(),
            file.format,
            file.app
        );
    }

    let mut views = file.views;
    if file.app != app {
        views.retain(|view, _| matches!(view, View::Palette | View::AgentSessions));
    }
    Ok(views)
}

/// `result`, kept as the latest `view` when it is an answer.
pub(crate) fn kept<T: Serialize>(
    cache: &Arc<ViewCache>,
    view: View,
    result: error::Result<T>,
) -> error::Result<T> {
    if let Ok(value) = &result {
        cache_view(cache, view, value);
    }

    result
}

/// Keeps `value` as the latest `view` for the next window that opens. The
/// file is written `WRITE_AFTER` later on a blocking thread, one write at a
/// time, and on quitting.
pub(crate) fn cache_view<T: Serialize>(cache: &Arc<ViewCache>, view: View, value: &T) {
    let value = match serde_json::to_value(value) {
        Ok(value) => value,
        Err(e) => {
            log::warn!("keeping the {view:?} view: {e}");
            return;
        }
    };

    if cache.put(view, value) {
        tauri::async_runtime::spawn(write_later(cache.clone()));
    }
}

/// Writes `cache` once `WRITE_AFTER` has passed, with every change made meanwhile.
async fn write_later(cache: Arc<ViewCache>) {
    tokio::time::sleep(WRITE_AFTER).await;

    if let Err(e) = tokio::task::spawn_blocking(move || cache.write_unsaved()).await {
        log::warn!("saving the views stopped: {e}");
    }
}

/// What the window last drew from the server, by view, for a window that
/// opens to show before the server answers; `None` until this Mac is linked.
#[tauri::command]
pub fn center_cached(
    beep: Service<'_>,
    cache: State<'_, Arc<ViewCache>>,
) -> Option<BTreeMap<View, Value>> {
    if cfg!(mobile) && beep.link_state() != LinkState::Linked {
        return None;
    }

    let mut views = cache.views();
    // This side's own head starts, not the page's.
    views.remove(&View::AgentSessions);
    views.remove(&View::Palette);
    views.remove(&View::GitItems);
    Some(views)
}

/// The most a project page cache may take, in bytes of JSON. The page caps
/// it lower; this only stops a runaway one filling the disk.
const MAX_PAGES: usize = 4 * 1024 * 1024;

/// Keeps project pages across windows and restarts. Desktop pages belong to
/// the local mesh; phone pages require a linked controller.
#[tauri::command]
pub fn center_keep_pages(
    beep: Service<'_>,
    cache: State<'_, Arc<ViewCache>>,
    pages: Value,
) -> error::Result<()> {
    if cfg!(mobile) && beep.link_state() != LinkState::Linked {
        return Err(DesktopError::Unlinked);
    }

    let size = serde_json::to_vec(&pages)
        .map_err(anyhow::Error::from)?
        .len();
    if size > MAX_PAGES {
        return Err(DesktopError::BadRequest(format!(
            "the page cache is {size} bytes, over the {MAX_PAGES} kept"
        )));
    }

    cache_view(&cache, View::Pages, &pages);
    Ok(())
}

#[cfg(test)]
#[path = "../tests/center_view_cache.rs"]
mod tests;
