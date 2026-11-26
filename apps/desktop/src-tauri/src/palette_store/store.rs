//! The lists, their tickets and the revision the palette follows. The lists are saved in the
//! views file, so a new launch finds them before the machines answer.

use super::lists::PaletteLists;
use crate::store::{
    lock,
    view_cache::{View, ViewCache, cache_view},
};
use beep_proto::{ImportedProject, MachineSession};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::watch;

pub(crate) struct PaletteStore {
    held: Mutex<PaletteLists>,
    /// Counts the reads of the mesh's projects and the removals, so a read
    /// taken before a removal cannot bring the project back.
    tickets: AtomicU64,
    /// Counts the changes to the lists.
    changes: watch::Sender<u64>,
}

impl PaletteStore {
    /// Starts from the lists saved in `views`, if it holds them.
    pub(crate) fn load(views: &ViewCache) -> Self {
        let mut lists: PaletteLists = match views.get(View::Palette).map(serde_json::from_value) {
            Some(Ok(lists)) => lists,
            Some(Err(e)) => {
                // Only a head start: the machines' lists replace it.
                log::info!("the saved palette lists do not parse: {e}");
                PaletteLists::default()
            }
            None => PaletteLists::default(),
        };
        lists.prune();

        Self {
            held: Mutex::new(lists),
            tickets: AtomicU64::new(0),
            changes: watch::channel(0).0,
        }
    }

    /// Wakes when the lists change.
    pub(crate) fn watch(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }

    /// Reads the lists.
    pub(crate) fn read<R>(&self, read: impl FnOnce(&PaletteLists) -> R) -> R {
        read(&lock(&self.held))
    }

    pub(crate) fn projects(&self) -> Vec<ImportedProject> {
        self.read(|lists| lists.projects.clone().unwrap_or_default())
    }

    /// Changes the lists, saves them and tells the followers.
    pub(crate) fn update(&self, app: &AppHandle, change: impl FnOnce(&mut PaletteLists)) {
        {
            let mut lists = lock(&self.held);
            change(&mut lists);
            lists.prune();
            cache_view(&app.state::<Arc<ViewCache>>(), View::Palette, &*lists);
        }
        self.changes.send_modify(|count| *count += 1);
    }

    /// The ticket of a read of the mesh's projects that starts now.
    pub(crate) fn ticket(&self) -> u64 {
        self.tickets.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// `projects` are the imports as the mesh held them at the read with ticket `read`.
    pub(crate) fn project_list(&self, app: &AppHandle, read: u64, projects: Vec<ImportedProject>) {
        let mut changed = false;
        self.update(app, |lists| changed = lists.read_projects(read, projects));

        if changed {
            let _ = app.emit("palette-text-changed", ());
        }
    }

    /// A machine's place of a project was removed: a read taken before now may still hold it.
    pub(crate) fn place_removed(&self, app: &AppHandle) {
        let removed_at = self.ticket();
        self.update(app, |lists| lists.removed_at = removed_at);
    }

    pub(crate) fn forget_project(&self, app: &AppHandle, project: &str) {
        let removed_at = self.ticket();
        self.update(app, |lists| {
            lists.removed_at = removed_at;
            lists.forget_project(project);
        });
        let _ = app.emit("palette-text-changed", ());
    }

    /// The user opened `session`: the palette finds it even when the mirror does not hold it.
    /// `mirrored` is true when the mirror already holds it.
    pub(crate) fn found_session(&self, app: &AppHandle, session: MachineSession, mirrored: bool) {
        let opened = self.read(|lists| {
            lists
                .opened
                .iter()
                .any(|s| s.machine_id == session.machine_id && s.id == session.id)
        });

        if !mirrored && !opened {
            self.update(app, |lists| lists.open(session));
        }
    }
}
