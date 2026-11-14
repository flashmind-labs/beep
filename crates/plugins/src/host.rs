use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Component, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use log::{debug, warn};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use thiserror::Error;

use crate::manifest::PluginId;
use crate::sandbox::{Sandbox, locked};
use crate::topic::{Delivery, Inbox, Publication, Topic, TopicError};
use crate::ui::Placement;

/// How long a plugin's files must stay quiet before it reloads.
const RELOAD_QUIET: Duration = Duration::from_millis(200);

/// The most `ui.*` topics whose latest value the host keeps.
const MAX_LOCAL_TOPICS: usize = 64;

/// Where the host reports changes. Calls come from the supervisor thread.
pub trait Handler: Send + Sync + 'static {
    /// Every placement of every running plugin, each time what is shown changes.
    fn shown(&self, placements: &[Placement]);

    /// The topics running plugins follow, each time that set changes.
    fn wanted(&self, topics: &[Topic]);
}

type Shared = Arc<dyn Handler>;

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
    wanted: Arc<Mutex<Vec<Topic>>>,
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
    /// The topics a plugin follows now.
    Wanted {
        id: PluginId,
        generation: u64,
        topics: Vec<Topic>,
    },
    /// An update from the page.
    Publish(Delivery),
    Stop,
}

impl Host {
    /// Loads every plugin under `dir` (created if missing), watches it, and reports to
    /// `handler` what is shown and which topics the plugins follow.
    pub fn start(dir: PathBuf, handler: impl Handler) -> Result<Host, HostError> {
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
        let wanted = Arc::new(Mutex::new(Vec::new()));
        let supervisor = Supervisor {
            dir,
            events: events.clone(),
            running: HashMap::new(),
            shown: BTreeMap::new(),
            reloads: HashMap::new(),
            published: Arc::clone(&shown),
            wanted: BTreeMap::new(),
            reported: Arc::clone(&wanted),
            local: HashMap::new(),
            handler: Arc::new(handler),
            generation: 0,
        };
        let handle = thread::Builder::new()
            .name("plugins-supervisor".to_owned())
            .spawn(move || supervisor.run(&inbox))
            .map_err(HostError::Spawn)?;

        Ok(Host {
            shown,
            wanted,
            events,
            supervisor: Some(handle),
            _watcher: watcher,
        })
    }

    /// What is shown now.
    pub fn placements(&self) -> Vec<Placement> {
        locked(&self.shown).clone()
    }

    /// The topics running plugins follow now.
    pub fn wanted(&self) -> Vec<Topic> {
        locked(&self.wanted).clone()
    }

    /// Hands an update to every plugin that follows its topic. The host keeps the
    /// latest value of each `ui.*` topic for plugins that subscribe later.
    pub fn publish(&self, publication: &Publication) -> Result<(), TopicError> {
        let delivery = Delivery::new(publication).inspect_err(|error| {
            warn!("dropped an update for {}: {error}", publication.topic.name);
        })?;
        let _ = self.events.send(Event::Publish(delivery));

        Ok(())
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
    /// Wakes the plugin's thread; dropping it stops the thread.
    wake: SyncSender<()>,
    inbox: Arc<Inbox>,
    thread: JoinHandle<()>,
    generation: u64,
}

impl Running {
    fn send(&self, delivery: Delivery) {
        self.inbox.push(delivery);
        // A full channel means a wake is already waiting.
        let _ = self.wake.try_send(());
    }
}

/// What one plugin follows, as its current run reported it.
struct Wanted {
    generation: u64,
    topics: Vec<Topic>,
}

/// Owns the running plugins. It is the only caller of the handler.
struct Supervisor {
    dir: PathBuf,
    events: Sender<Event>,
    running: HashMap<PluginId, Running>,
    shown: BTreeMap<PluginId, Vec<Placement>>,
    reloads: HashMap<PluginId, Instant>,
    published: Arc<Mutex<Vec<Placement>>>,
    wanted: BTreeMap<PluginId, Wanted>,
    reported: Arc<Mutex<Vec<Topic>>>,
    /// The latest update of each `ui.*` topic.
    local: HashMap<Topic, Delivery>,
    handler: Shared,
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
                Some(Event::Wanted {
                    id,
                    generation,
                    topics,
                }) => self.note_wanted(id, generation, topics),
                Some(Event::Publish(delivery)) => self.route(delivery),
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

    fn note_wanted(&mut self, id: PluginId, generation: u64, topics: Vec<Topic>) {
        let Some(running) = self.running.get(&id) else {
            return;
        };
        if running.generation != generation {
            return;
        }

        // A plugin that just subscribed to a `ui.*` topic gets its latest value.
        let known = self
            .wanted
            .get(&id)
            .filter(|held| held.generation == generation);
        for topic in topics.iter().filter(|topic| topic.is_local()) {
            let is_new = known.is_none_or(|held| !held.topics.contains(topic));
            if is_new && let Some(latest) = self.local.get(topic) {
                running.send(latest.clone());
            }
        }

        self.wanted.insert(id, Wanted { generation, topics });
        self.sync_wanted();
    }

    /// Gives an update to each running plugin that follows its topic.
    fn route(&mut self, delivery: Delivery) {
        if delivery.topic.is_local() {
            if self.local.len() >= MAX_LOCAL_TOPICS && !self.local.contains_key(&delivery.topic) {
                warn!("too many ui topics, dropped {}", delivery.topic.name);
                return;
            }
            self.local.insert(delivery.topic.clone(), delivery.clone());
        }

        for (id, wanted) in &self.wanted {
            let running = self.running.get(id);
            if let Some(running) = running
                && running.generation == wanted.generation
                && wanted.topics.contains(&delivery.topic)
            {
                running.send(delivery.clone());
            }
        }
    }

    /// Reports the topics all plugins follow when they changed.
    fn sync_wanted(&self) {
        let all: BTreeSet<&Topic> = self
            .wanted
            .values()
            .flat_map(|wanted| wanted.topics.iter())
            .collect();
        let all: Vec<Topic> = all.into_iter().cloned().collect();
        {
            let mut reported = locked(&self.reported);
            if *reported == all {
                return;
            }
            reported.clone_from(&all);
        }

        self.handler.wanted(&all);
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
            } else {
                self.wanted.remove(&id);
                self.sync_wanted();
                if self.shown.remove(&id).is_some() {
                    self.publish();
                }
            }
        }
    }

    fn start_plugin(&mut self, id: PluginId) {
        self.stop_plugin(&id);
        self.generation += 1;
        let generation = self.generation;
        let (wake, woken) = mpsc::sync_channel(1);
        let inbox = Arc::new(Inbox::default());
        let job = PluginJob {
            dir: self.dir.join(id.to_string()),
            id: id.clone(),
            generation,
            events: self.events.clone(),
            inbox: Arc::clone(&inbox),
        };

        let spawned = thread::Builder::new()
            .name(format!("plugin-{id}"))
            .spawn(move || job.run(&woken));
        match spawned {
            Ok(thread) => {
                self.running.insert(
                    id,
                    Running {
                        wake,
                        inbox,
                        thread,
                        generation,
                    },
                );
            }
            Err(error) => {
                warn!("cannot start plugin {id}: {error}");
                self.wanted.remove(&id);
                self.sync_wanted();
            }
        }
    }

    /// Stops a plugin's thread and waits for it. What it showed and followed stays until replaced.
    fn stop_plugin(&mut self, id: &PluginId) {
        if let Some(running) = self.running.remove(id) {
            drop(running.wake);
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

        self.handler.shown(&all);
    }
}

/// One plugin's thread: render, wait for the next timer or a stop, fire.
struct PluginJob {
    dir: PathBuf,
    id: PluginId,
    generation: u64,
    events: Sender<Event>,
    inbox: Arc<Inbox>,
}

impl PluginJob {
    fn run(self, woken: &Receiver<()>) {
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

            if let Some(topics) = sandbox.take_wanted()
                && !self.report(topics)
            {
                return;
            }

            let woke = match sandbox.next_timer() {
                Some(due) => woken.recv_timeout(due.saturating_duration_since(Instant::now())),
                None => woken.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match woke {
                Err(RecvTimeoutError::Timeout) => {
                    if let Err(error) = sandbox.fire_due(Instant::now()) {
                        return self.fail(&error);
                    }
                }
                Ok(()) => {
                    for delivery in self.inbox.take() {
                        if let Err(error) = sandbox.deliver(&delivery) {
                            return self.fail(&error);
                        }
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    /// Logs the error once and shows nothing for this plugin.
    fn fail(&self, error: &crate::sandbox::SandboxError) {
        warn!("plugin {} failed: {error}", self.id);
        self.show(Vec::new());
        self.report(Vec::new());
    }

    /// Returns false when the supervisor is gone.
    fn report(&self, topics: Vec<Topic>) -> bool {
        self.events
            .send(Event::Wanted {
                id: self.id.clone(),
                generation: self.generation,
                topics,
            })
            .is_ok()
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
