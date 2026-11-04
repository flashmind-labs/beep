//! The views the window last drew from the server, in one file, so a new
//! window shows them at once. Only what the server lists goes in, never a
//! key; a file of another format or app version is dropped and rewritten.

use crate::{
    beep::{Beep, state::LinkState},
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
};
use tauri::State;

/// Raised whenever the file's own layout changes.
const FORMAT: u32 = 1;

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

    /// Writes what is held now, through a partial file so a reader never
    /// sees half of it.
    fn save(&self) -> anyhow::Result<()> {
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
        Ok(())
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
    if file.format != FORMAT || file.app != app {
        bail!(
            "{} is from format {} of app {}",
            path.display(),
            file.format,
            file.app
        );
    }
    Ok(file.views)
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
/// file is written on a blocking thread, one write at a time.
pub(crate) fn cache_view<T: Serialize>(cache: &Arc<ViewCache>, view: View, value: &T) {
    let value = match serde_json::to_value(value) {
        Ok(value) => value,
        Err(e) => {
            log::warn!("keeping the {view:?} view: {e}");
            return;
        }
    };
    if cache.put(view, value) {
        let cache = cache.clone();
        tauri::async_runtime::spawn_blocking(move || cache.write_unsaved());
    }
}

/// What the window last drew from the server, by view, for a window that
/// opens to show before the server answers; `None` until this Mac is linked.
#[tauri::command]
pub fn center_cached(
    beep: State<'_, Arc<Beep>>,
    cache: State<'_, Arc<ViewCache>>,
) -> Option<BTreeMap<View, Value>> {
    if beep.link_state() != LinkState::Linked {
        return None;
    }
    let mut views = cache.views();
    // Home's own head start, not the page's.
    views.remove(&View::AgentSessions);
    Some(views)
}

/// The most a project page cache may take, in bytes of JSON. The page caps
/// it lower; this only stops a runaway one filling the disk.
const MAX_PAGES: usize = 4 * 1024 * 1024;

/// Keeps what the project pages last read, so the next window, even after
/// a restart, draws them at once. Only while linked: the file goes with
/// the account on Start over.
#[tauri::command]
pub fn center_keep_pages(
    beep: State<'_, Arc<Beep>>,
    cache: State<'_, Arc<ViewCache>>,
    pages: Value,
) -> error::Result<()> {
    if beep.link_state() != LinkState::Linked {
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
mod tests {
    use super::*;
    use serde_json::json;
    use uuid::Uuid;

    /// A path in a fresh directory of its own.
    fn path() -> PathBuf {
        std::env::temp_dir()
            .join(format!("beep-views-{}", Uuid::new_v4()))
            .join("views.json")
    }

    #[test]
    fn a_missing_file_starts_empty() {
        let cache = ViewCache::load(path(), "1.0.0");
        assert!(cache.views().is_empty());
    }

    #[test]
    fn saved_views_come_back() {
        let path = path();
        let cache = ViewCache::load(path.clone(), "1.0.0");
        assert!(cache.put(View::Home, json!({"machines": []})));
        cache.write_unsaved();
        assert!(cache.put(View::Top, json!(["org/repo"])));
        cache.write_unsaved();

        let again = ViewCache::load(path.clone(), "1.0.0");
        assert_eq!(again.get(View::Home), Some(json!({"machines": []})));
        assert_eq!(again.get(View::Top), Some(json!(["org/repo"])));
        assert_eq!(again.get(View::Groups), None);
        again.clear();
        assert!(!path.exists());
        assert!(again.views().is_empty());
    }

    #[test]
    fn an_unchanged_view_is_not_written_again() {
        let cache = ViewCache::load(path(), "1.0.0");
        assert!(cache.put(View::Sync, json!([])));
        // A writer is due and has not run: it will write this one too.
        assert!(!cache.put(View::Top, json!([])));
        cache.write_unsaved();
        assert!(!cache.put(View::Sync, json!([])));
        assert!(cache.put(View::Sync, json!([1])));
        cache.write_unsaved();
        cache.clear();
    }

    #[test]
    fn another_version_is_dropped() {
        let path = path();
        let cache = ViewCache::load(path.clone(), "1.0.0");
        cache.put(View::Home, json!({}));
        cache.write_unsaved();
        assert!(ViewCache::load(path.clone(), "1.0.1").views().is_empty());

        let old = json!({"format": FORMAT + 1, "app": "1.0.0", "views": {"home": {}}});
        fs::write(&path, old.to_string()).expect("the file is written");
        assert!(ViewCache::load(path.clone(), "1.0.0").views().is_empty());
        cache.clear();
    }

    #[test]
    fn a_broken_file_is_dropped() {
        let path = path();
        let cache = ViewCache::load(path.clone(), "1.0.0");
        cache.put(View::Home, json!({}));
        cache.write_unsaved();
        for broken in [&b"{\"format\":1,"[..], b"", b"\x00\xff", b"{\"views\":{}}"] {
            fs::write(&path, broken).expect("the file is written");
            assert!(ViewCache::load(path.clone(), "1.0.0").views().is_empty());
        }
        // One naming a view this version does not know.
        let unknown = json!({"format": FORMAT, "app": "1.0.0", "views": {"later": {}}});
        fs::write(&path, unknown.to_string()).expect("the file is written");
        assert!(ViewCache::load(path.clone(), "1.0.0").views().is_empty());
        // Writing again replaces it.
        let fresh = ViewCache::load(path.clone(), "1.0.0");
        fresh.put(View::Home, json!({"a": 1}));
        fresh.write_unsaved();
        assert_eq!(
            ViewCache::load(path.clone(), "1.0.0").get(View::Home),
            Some(json!({"a": 1}))
        );
        fresh.clear();
    }
}
