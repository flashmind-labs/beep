//! Filesystem watches that follow missing directories without watching whole ancestors recursively.

use anyhow::{Context, Result, bail};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::{sync::mpsc, task::spawn_blocking};
use tokio_util::task::AbortOnDropHandle;

type Changed = Arc<dyn Fn(Vec<PathBuf>, bool) + Send + Sync>;

pub(crate) struct Paths {
    inner: Arc<Mutex<Inner>>,
    _reconcile: AbortOnDropHandle<()>,
    changed: Changed,
    #[cfg(test)]
    reconciled: tokio::sync::watch::Receiver<()>,
}

struct Inner {
    watcher: RecommendedWatcher,
    stale: Arc<AtomicBool>,
    roots: Arc<Mutex<Vec<PathBuf>>>,
    installed: BTreeMap<PathBuf, RecursiveMode>,
}

impl Paths {
    pub(crate) fn new(changed: impl Fn(Vec<PathBuf>) + Send + Sync + 'static) -> Result<Self> {
        Self::with_structure(move |roots, _| changed(roots))
    }

    pub(crate) fn with_structure(
        changed: impl Fn(Vec<PathBuf>, bool) + Send + Sync + 'static,
    ) -> Result<Self> {
        let roots = Arc::new(Mutex::new(Vec::<PathBuf>::new()));
        let interested = roots.clone();
        let stale = Arc::new(AtomicBool::new(false));
        let invalidated = stale.clone();
        let (refresh, mut refreshes) = mpsc::channel(1);
        let changed: Changed = Arc::new(changed);
        let reconciled_callback = changed.clone();
        let direct = changed.clone();
        #[cfg(test)]
        let (reconciled_tx, reconciled) = tokio::sync::watch::channel(());
        let watcher = RecommendedWatcher::new(
            move |event: notify::Result<notify::Event>| {
                let roots = crate::lock(&interested);
                let changed_roots = match &event {
                    Ok(event) if matches!(event.kind, EventKind::Access(_) | EventKind::Other) => {
                        return;
                    }
                    Ok(event) => roots
                        .iter()
                        .filter(|root| event.paths.iter().any(|path| affects(root, path)))
                        .cloned()
                        .collect::<Vec<_>>(),
                    Err(_) => roots.clone(),
                };
                if event.as_ref().map_or(true, |event| {
                    matches!(
                        event.kind,
                        EventKind::Remove(_)
                            | EventKind::Modify(notify::event::ModifyKind::Name(_))
                    ) && event
                        .paths
                        .iter()
                        .any(|path| roots.iter().any(|root| root.starts_with(path)))
                }) {
                    invalidated.store(true, Ordering::Release);
                }
                drop(roots);
                if changed_roots.is_empty() {
                    return;
                }
                let structural = event.as_ref().map_or(true, |event| {
                    matches!(
                        event.kind,
                        EventKind::Create(_)
                            | EventKind::Remove(_)
                            | EventKind::Modify(notify::event::ModifyKind::Name(_))
                    )
                });
                direct(changed_roots, structural);
                if structural {
                    let _ = refresh.try_send(());
                }
            },
            notify::Config::default(),
        )?;
        let inner = Arc::new(Mutex::new(Inner {
            watcher,
            stale,
            roots,
            installed: BTreeMap::new(),
        }));
        let weak = Arc::downgrade(&inner);
        let reconcile = AbortOnDropHandle::new(tokio::spawn(async move {
            while refreshes.recv().await.is_some() {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let (result, roots) = match spawn_blocking(move || {
                    let mut held = crate::lock(&inner);
                    let roots = crate::lock(&held.roots).clone();
                    (held.install(&roots), roots)
                })
                .await
                {
                    Ok(done) => done,
                    Err(_) => return,
                };
                if result.is_err() {
                    log::debug!("filesystem notification watches need reconciliation");
                }
                // Rebuilding an OS watch can leave a short gap; reconcile state afterwards.
                if !matches!(result, Ok(false)) {
                    reconciled_callback(roots, true);
                }
                #[cfg(test)]
                reconciled_tx.send_replace(());
            }
        }));
        Ok(Self {
            inner,
            _reconcile: reconcile,
            changed,
            #[cfg(test)]
            reconciled,
        })
    }

    pub(crate) async fn replace(&self, mut roots: Vec<PathBuf>) -> Result<()> {
        roots.sort();
        roots.dedup();
        let inner = self.inner.clone();
        let (result, changed) = spawn_blocking(move || {
            let mut inner = crate::lock(&inner);
            let before = crate::lock(&inner.roots).clone();
            let result = inner.install(&roots);
            if result.is_ok() {
                *crate::lock(&inner.roots) = roots.clone();
            }
            let changed = if before != roots || !matches!(result, Ok(false)) {
                before.into_iter().chain(roots).collect()
            } else {
                Vec::new()
            };
            (result, changed)
        })
        .await
        .context("filesystem watcher stopped")?;
        if !changed.is_empty() {
            (self.changed)(changed, true);
        }
        result.map(|_| ())
    }
}

impl Inner {
    fn install(&mut self, roots: &[PathBuf]) -> Result<bool> {
        let mut wanted = BTreeMap::new();
        for root in roots {
            let (path, mode) = existing(root)?;
            wanted
                .entry(path)
                .and_modify(|held| {
                    if mode == RecursiveMode::Recursive {
                        *held = mode;
                    }
                })
                .or_insert(mode);
        }
        // A recursive parent already covers its descendants.
        let recursive: Vec<_> = wanted
            .iter()
            .filter(|(_, mode)| **mode == RecursiveMode::Recursive)
            .map(|(path, _)| path.clone())
            .collect();
        wanted.retain(|path, _| {
            !recursive
                .iter()
                .any(|parent| parent != path && path.starts_with(parent))
        });
        let stale = self.stale.swap(false, Ordering::AcqRel);
        if self.installed == wanted && !stale {
            return Ok(false);
        }
        // notify owns OS watch descriptors by path, without caller reference counts.
        // Remove the previous tree before rebuilding overlapping roots.
        for path in self.installed.keys() {
            let _ = self.watcher.unwatch(path);
        }
        let mut added: Vec<PathBuf> = Vec::new();
        for (path, mode) in &wanted {
            if let Err(error) = self.watcher.watch(path, *mode) {
                for path in added {
                    let _ = self.watcher.unwatch(&path);
                }
                for (path, mode) in &self.installed {
                    let _ = self.watcher.watch(path, *mode);
                }
                self.stale.store(true, Ordering::Release);
                return Err(error.into());
            }
            added.push(path.clone());
        }
        self.installed = wanted;
        Ok(true)
    }
}

fn affects(root: &Path, path: &Path) -> bool {
    if let Ok(relative) = path.strip_prefix(root) {
        return !relative.components().any(|component| {
            matches!(
                component.as_os_str().to_str(),
                Some("node_modules" | "target" | ".worktrees")
            )
        });
    }
    root.starts_with(path)
}

fn existing(root: &Path) -> Result<(PathBuf, RecursiveMode)> {
    if root.exists() {
        return Ok((root.to_owned(), RecursiveMode::Recursive));
    }
    for parent in root.ancestors().skip(1) {
        if parent.is_dir() {
            return Ok((parent.to_owned(), RecursiveMode::NonRecursive));
        }
    }
    bail!("the watched path has no existing parent")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_missing_directory_only_watches_its_parent_non_recursively() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("new/store");
        let watcher = Paths::new(|_| {}).unwrap();
        watcher.replace(vec![path]).await.unwrap();
        let held = crate::lock(&watcher.inner);
        assert_eq!(held.installed.len(), 1);
        assert_eq!(
            held.installed.get(root.path()),
            Some(&RecursiveMode::NonRecursive)
        );
    }

    #[tokio::test]
    async fn aliases_share_one_watch_and_removing_one_keeps_the_directory_watched() {
        let root = tempfile::tempdir().unwrap();
        let (changed, mut changes) = mpsc::channel(8);
        let watcher = Paths::new(move |paths| {
            let _ = changed.try_send(paths);
        })
        .unwrap();
        watcher
            .replace(vec![root.path().to_owned(), root.path().to_owned()])
            .await
            .unwrap();
        watcher.replace(vec![root.path().to_owned()]).await.unwrap();
        assert_eq!(crate::lock(&watcher.inner).installed.len(), 1);
        std::fs::write(root.path().join("example.txt"), "changed").unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), changes.recv())
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn renaming_and_recreating_a_root_rearms_the_replacement_directory() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("watched");
        std::fs::create_dir(&root).unwrap();
        let (changed, mut changes) = mpsc::channel(32);
        let watcher = Paths::new(move |paths| {
            let _ = changed.try_send(paths);
        })
        .unwrap();
        watcher.replace(vec![root.clone()]).await.unwrap();
        while changes.try_recv().is_ok() {}
        let inner = watcher.inner.clone();
        let (ready, wait_ready) = tokio::sync::oneshot::channel();
        let (release, wait_release) = std::sync::mpsc::channel();
        let original = root.clone();
        let renamed = directory.path().join("old");
        let hold = tokio::task::spawn_blocking(move || {
            let _held = crate::lock(&inner);
            std::fs::rename(&original, renamed).unwrap();
            std::fs::create_dir(&original).unwrap();
            ready.send(()).unwrap();
            wait_release
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
        });
        wait_ready.await.unwrap();
        timeout_change(&mut changes).await;
        let mut reconciled = watcher.reconciled.clone();
        reconciled.borrow_and_update();
        release.send(()).unwrap();
        hold.await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), reconciled.changed())
            .await
            .unwrap()
            .unwrap();
        while changes.try_recv().is_ok() {}
        std::fs::write(root.join("after.txt"), "changed").unwrap();
        timeout_change(&mut changes).await;
    }

    async fn timeout_change(changes: &mut mpsc::Receiver<Vec<PathBuf>>) {
        tokio::time::timeout(std::time::Duration::from_secs(5), changes.recv())
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn a_directory_created_after_subscribing_becomes_recursive() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("new/store");
        let (changed, mut changes) = mpsc::channel(32);
        let watcher = Paths::new(move |paths| {
            let _ = changed.try_send(paths);
        })
        .unwrap();
        watcher.replace(vec![path.clone()]).await.unwrap();
        std::fs::create_dir_all(&path).unwrap();
        let mut reconciled = watcher.reconciled.clone();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                reconciled.borrow_and_update();
                if crate::lock(&watcher.inner).installed.get(&path)
                    == Some(&RecursiveMode::Recursive)
                {
                    break;
                }
                reconciled.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        std::fs::write(path.join("example.txt"), "changed").unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), changes.recv())
            .await
            .unwrap()
            .unwrap();
    }
}
