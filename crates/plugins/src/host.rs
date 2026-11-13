use std::collections::{BTreeMap, HashMap};
use std::path::{Component, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use log::{debug, warn};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use thiserror::Error;

use crate::manifest::PluginId;
use crate::sandbox::{Sandbox, locked};
use crate::ui::Placement;

/// How long a plugin's files must stay quiet before it reloads.
const RELOAD_QUIET: Duration = Duration::from_millis(200);

type Changed = Arc<dyn Fn(&[Placement]) + Send + Sync>;

#[derive(Debug, Error)]
pub enum HostError {
    #[error("cannot prepare plugins folder {}: {source}", path.display())]
    Folder {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot watch plugins folder: {0}")]
    Watch(#[from] notify::Error),
    #[error("cannot start plugin supervisor: {0}")]
    Spawn(std::io::Error),
}

/// Runs every plugin in a folder and reloads them when their files change.
/// Dropping it stops all of its threads.
pub struct Host {
    shown: Arc<Mutex<Vec<Placement>>>,
    events: Sender<Event>,
    supervisor: Option<JoinHandle<()>>,
    _watcher: RecommendedWatcher,
}

enum Event {
    /// A path changed under the plugins folder.
    Fs(PathBuf),
    /// A plugin's render result. Empty also means the plugin failed.
    Shown {
        id: PluginId,
        generation: u64,
        placements: Vec<Placement>,
    },
    Stop,
}

impl Host {
    /// Loads every plugin under `dir` (created if missing), watches it, and calls `changed`
    /// with every placement of every running plugin each time what is shown changes.
    pub fn start(
        dir: PathBuf,
        changed: impl Fn(&[Placement]) + Send + Sync + 'static,
    ) -> Result<Host, HostError> {
        let folder = |source| HostError::Folder {
            path: dir.clone(),
            source,
        };
        std::fs::create_dir_all(&dir).map_err(folder)?;
        let dir = dir.canonicalize().map_err(folder)?;

        let (events, inbox) = mpsc::channel();
        let sender = events.clone();
        let mut watcher = notify::recommended_watcher(
            move |result: notify::Result<notify::Event>| match result {
                Ok(event) if !matches!(event.kind, EventKind::Access(_)) => {
                    for path in event.paths {
                        let _ = sender.send(Event::Fs(path));
                    }
                }
                Ok(_) => {}
                Err(error) => warn!("plugin folder watcher: {error}"),
            },
        )?;
        watcher.watch(&dir, RecursiveMode::Recursive)?;

        let shown = Arc::new(Mutex::new(Vec::new()));
        let supervisor = Supervisor {
            dir,
            events: events.clone(),
            running: HashMap::new(),
            shown: BTreeMap::new(),
            reloads: HashMap::new(),
            published: Arc::clone(&shown),
            changed: Arc::new(changed),
            generation: 0,
        };
        let handle = thread::Builder::new()
            .name("plugins-supervisor".to_owned())
            .spawn(move || supervisor.run(&inbox))
            .map_err(HostError::Spawn)?;

        Ok(Host {
            shown,
            events,
            supervisor: Some(handle),
            _watcher: watcher,
        })
    }

    /// What is shown now.
    pub fn placements(&self) -> Vec<Placement> {
        locked(&self.shown).clone()
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.events.send(Event::Stop);
        if let Some(handle) = self.supervisor.take() {
            let _ = handle.join();
        }
    }
}

struct Running {
    stop: Sender<()>,
    thread: JoinHandle<()>,
    generation: u64,
}

/// Owns the running plugins. It is the only caller of `changed`.
struct Supervisor {
    dir: PathBuf,
    events: Sender<Event>,
    running: HashMap<PluginId, Running>,
    shown: BTreeMap<PluginId, Vec<Placement>>,
    reloads: HashMap<PluginId, Instant>,
    published: Arc<Mutex<Vec<Placement>>>,
    changed: Changed,
    generation: u64,
}

impl Supervisor {
    fn run(mut self, inbox: &Receiver<Event>) {
        self.load_all();

        loop {
            let next_reload = self.reloads.values().min().copied();
            let event = match next_reload {
                Some(due) => {
                    match inbox.recv_timeout(due.saturating_duration_since(Instant::now())) {
                        Ok(event) => Some(event),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
                None => match inbox.recv() {
                    Ok(event) => Some(event),
                    Err(_) => break,
                },
            };

            match event {
                Some(Event::Stop) => break,
                Some(Event::Fs(path)) => self.note_change(&path),
                Some(Event::Shown {
                    id,
                    generation,
                    placements,
                }) => self.note_shown(id, generation, placements),
                None => {}
            }

            self.reload_due();
        }

        let ids: Vec<PluginId> = self.running.keys().cloned().collect();
        for id in ids {
            self.stop_plugin(&id);
        }
    }

    fn load_all(&mut self) {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) => {
                warn!("cannot list plugins folder: {error}");
                return;
            }
        };

        for entry in entries.flatten() {
            let id = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse().ok());
            if let Some(id) = id
                && entry.path().is_dir()
            {
                self.start_plugin(id);
            }
        }
    }

    /// Queues a reload for the plugin that owns `path`.
    fn note_change(&mut self, path: &std::path::Path) {
        let first = path
            .strip_prefix(&self.dir)
            .ok()
            .and_then(|rest| rest.components().next());
        let Some(Component::Normal(name)) = first else {
            return;
        };
        match name.to_str().map(str::parse::<PluginId>) {
            Some(Ok(id)) => {
                self.reloads.insert(id, Instant::now() + RELOAD_QUIET);
            }
            _ => debug!("ignoring change to {}", path.display()),
        }
    }

    fn note_shown(&mut self, id: PluginId, generation: u64, placements: Vec<Placement>) {
        let current = self.running.get(&id).map(|running| running.generation);
        if current != Some(generation) {
            return;
        }

        self.shown.insert(id, placements);
        self.publish();
    }

    fn reload_due(&mut self) {
        let now = Instant::now();
        let due: Vec<PluginId> = self
            .reloads
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(id, _)| id.clone())
            .collect();

        for id in due {
            self.reloads.remove(&id);
            self.stop_plugin(&id);

            if self.dir.join(id.to_string()).is_dir() {
                self.start_plugin(id);
            } else if self.shown.remove(&id).is_some() {
                self.publish();
            }
        }
    }

    fn start_plugin(&mut self, id: PluginId) {
        self.stop_plugin(&id);
        self.generation += 1;
        let generation = self.generation;
        let (stop, stopped) = mpsc::channel();
        let job = PluginJob {
            dir: self.dir.join(id.to_string()),
            id: id.clone(),
            generation,
            events: self.events.clone(),
        };

        let spawned = thread::Builder::new()
            .name(format!("plugin-{id}"))
            .spawn(move || job.run(&stopped));
        match spawned {
            Ok(thread) => {
                self.running.insert(
                    id,
                    Running {
                        stop,
                        thread,
                        generation,
                    },
                );
            }
            Err(error) => warn!("cannot start plugin {id}: {error}"),
        }
    }

    /// Stops a plugin's thread and waits for it. What it showed stays until replaced.
    fn stop_plugin(&mut self, id: &PluginId) {
        if let Some(running) = self.running.remove(id) {
            drop(running.stop);
            let _ = running.thread.join();
        }
    }

    fn publish(&self) {
        let all: Vec<Placement> = self.shown.values().flatten().cloned().collect();
        {
            let mut published = locked(&self.published);
            if *published == all {
                return;
            }
            published.clone_from(&all);
        }

        (self.changed)(&all);
    }
}

/// One plugin's thread: render, wait for the next timer or a stop, fire.
struct PluginJob {
    dir: PathBuf,
    id: PluginId,
    generation: u64,
    events: Sender<Event>,
}

impl PluginJob {
    fn run(self, stopped: &Receiver<()>) {
        let mut sandbox = match Sandbox::load(&self.dir, Instant::now()) {
            Ok(sandbox) => sandbox,
            Err(error) => return self.fail(&error),
        };

        loop {
            match sandbox.render() {
                Ok(Some(placements)) => {
                    if !self.show(placements) {
                        return;
                    }
                }
                Ok(None) => {}
                Err(error) => return self.fail(&error),
            }

            let woke = match sandbox.next_timer() {
                Some(due) => stopped.recv_timeout(due.saturating_duration_since(Instant::now())),
                None => stopped.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match woke {
                Err(RecvTimeoutError::Timeout) => {
                    if let Err(error) = sandbox.fire_due(Instant::now()) {
                        return self.fail(&error);
                    }
                }
                Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    /// Logs the error once and shows nothing for this plugin.
    fn fail(&self, error: &crate::sandbox::SandboxError) {
        warn!("plugin {} failed: {error}", self.id);
        self.show(Vec::new());
    }

    /// Returns false when the supervisor is gone.
    fn show(&self, placements: Vec<Placement>) -> bool {
        self.events
            .send(Event::Shown {
                id: self.id.clone(),
                generation: self.generation,
                placements,
            })
            .is_ok()
    }
}

#[cfg(test)]
#[path = "tests/host.rs"]
mod tests;
