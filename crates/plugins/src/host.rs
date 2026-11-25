use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use log::{Level, debug, warn};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use thiserror::Error;

use crate::asset::{Asset, AssetHash};
use crate::console::emit;
use crate::event::{EventError, Limiter, Mailbox, UiEvent};
use crate::install::{Builtin, ManageError, Preview};
use crate::instance::{InstanceBatch, InstanceError, Instances};
use crate::job::{Event, PluginJob};
use crate::lock::locked;
use crate::managed::{Managed, Origin, Policy, Record, Verdict};
use crate::manifest::{Activation, Manifest, PluginId};
use crate::meter::{BUDGET, Meter};
use crate::status::{Info, Lifecycle, PluginState, PluginStatus, Registry};
use crate::store::DATA_DIR;
use crate::style::Style;
use crate::tool::{MAX_JSON_BYTES, TIMEOUT, ToolCall, ToolError, ToolName};
use crate::topic::{Delivery, Inbox, Publication, Topic, TopicError};
use crate::ui::Placement;

/// How long a plugin's files must stay quiet before it reloads.
const RELOAD_QUIET: Duration = Duration::from_millis(200);

/// How long a slot-activated plugin keeps running after its slots left the page.
pub(crate) const IDLE_GRACE: Duration = Duration::from_secs(60);

/// How long past a tool's own deadline `Host::call_tool` waits for the plugin's thread.
const CALL_GRACE: Duration = Duration::from_secs(2);

/// The most `ui.*` topics whose latest value the host keeps.
const MAX_LOCAL_TOPICS: usize = 64;

/// Where the host reports changes. Calls come from the supervisor thread.
pub trait Handler: Send + Sync + 'static {
    /// Every placement of every running plugin, each time what is shown changes.
    fn shown(&self, placements: &[Placement]);

    /// The topics running plugins follow, each time that set changes.
    fn wanted(&self, topics: &[Topic]);

    /// Something in the plugins' list or states changed, such as a plugin turned
    /// off, stopped or installed. Read it with `Host::status`. CPU time and
    /// pending timers do not count.
    fn status_changed(&self) {}
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

/// What the host publishes for readers on any thread.
#[derive(Default)]
pub(crate) struct Published {
    shown: Mutex<Vec<Placement>>,
    wanted: Mutex<Vec<Topic>>,
    assets: Mutex<HashMap<AssetHash, Asset>>,
    registry: Mutex<Registry>,
}

/// Runs every plugin in a folder and reloads them when their files change.
/// Dropping it stops all of its threads.
pub struct Host {
    dir: PathBuf,
    managed: Arc<Managed>,
    managing: Mutex<()>,
    published: Arc<Published>,
    events: Sender<Event>,
    supervisor: Option<JoinHandle<()>>,
    _watcher: RecommendedWatcher,
}

impl Host {
    /// Loads every plugin under `dir` (created if missing), watches it, and reports to
    /// `handler` what is shown and which topics the plugins follow. Plugins that start
    /// at launch are running when it returns; the others wait for their activation.
    /// Plugins stay paused until a window reports that it is visible. `policy` says
    /// whether a plugin with no record runs. `builtins` ship with the app: they are
    /// installed and allowed here (not under `Policy::Dev`) and cannot be removed.
    pub fn start(
        dir: PathBuf,
        handler: impl Handler,
        policy: Policy,
        builtins: &[Builtin],
    ) -> Result<Host, HostError> {
        let folder = |source| HostError::Folder {
            path: dir.clone(),
            source,
        };
        std::fs::create_dir_all(&dir).map_err(folder)?;
        let dir = dir.canonicalize().map_err(folder)?;
        let managed =
            Managed::load(&dir, policy).with_builtins(builtins.iter().map(|b| b.id.clone()));
        managed.recover_installs().map_err(folder)?;
        managed.sync_builtins(&dir, builtins);
        let managed = Arc::new(managed);

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

        let published = Arc::new(Published::default());
        let mut supervisor = Supervisor::new(
            dir.clone(),
            Arc::clone(&managed),
            events.clone(),
            Arc::new(handler),
            Arc::clone(&published),
        );
        let (ready, loaded) = mpsc::channel();
        let handle = thread::Builder::new()
            .name("plugins-supervisor".to_owned())
            .spawn(move || {
                supervisor.load_all(Instant::now());
                let _ = ready.send(());
                supervisor.run(&inbox);
            })
            .map_err(HostError::Spawn)?;
        let _ = loaded.recv();

        Ok(Host {
            dir,
            managed,
            managing: Mutex::new(()),
            published,
            events,
            supervisor: Some(handle),
            _watcher: watcher,
        })
    }

    /// What is shown now.
    pub fn placements(&self) -> Vec<Placement> {
        locked(&self.published.shown).clone()
    }

    /// The topics running plugins follow now.
    pub fn wanted(&self) -> Vec<Topic> {
        locked(&self.published.wanted).clone()
    }

    /// The image a shown placement names, or none when no placement uses it now.
    pub fn asset(&self, hash: &AssetHash) -> Option<Asset> {
        locked(&self.published.assets).get(hash).cloned()
    }

    /// Every plugin's state.
    pub fn status(&self) -> Vec<PluginStatus> {
        locked(&self.published.registry).list()
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

    /// Says whether the window with this label is visible. While no window is,
    /// plugins draw nothing: their timers wait and updates pile up, the newest
    /// of each topic kept. When one shows, overdue timers fire once each.
    pub fn set_visible(&self, window: &str, visible: bool) {
        let _ = self.events.send(Event::Visible {
            window: window.to_owned(),
            visible,
        });
    }
}

impl Host {
    /// The folder plugins are installed in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn policy(&self) -> Policy {
        self.managed.policy()
    }

    /// What the host recorded about a plugin.
    pub fn record(&self, id: &PluginId) -> Option<Record> {
        self.managed.record(id)
    }

    /// Turns a plugin on or off. Its status shows the change when this returns.
    pub fn set_enabled(&self, id: &PluginId, enabled: bool) -> Result<(), ManageError> {
        let managing = locked(&self.managing);
        let manifest = self.installed_manifest(id)?;
        self.managed
            .update(id, Some(&manifest), |record| record.enabled = enabled)
            .map_err(ManageError::io("cannot save the plugin's record"))?;
        drop(managing);
        self.refresh(id);

        Ok(())
    }

    /// Allows every permission the plugin's manifest asks for, and turns it on.
    /// Only the user may call this, from the page that shows what is asked.
    pub fn approve(&self, id: &PluginId) -> Result<(), ManageError> {
        let managing = locked(&self.managing);
        let manifest = self.installed_manifest(id)?;
        let approved = crate::managed::sorted(&manifest.permissions);
        self.managed
            .update(id, Some(&manifest), |record| {
                record.enabled = true;
                record.approved = approved;
            })
            .map_err(ManageError::io("cannot save the plugin's record"))?;
        drop(managing);
        self.refresh(id);

        Ok(())
    }

    /// Stops a plugin, then deletes its folder and its record.
    pub fn remove(&self, id: &PluginId) -> Result<(), ManageError> {
        let managing = locked(&self.managing);
        let folder = self.dir.join(id.to_string());
        let record = self.managed.record(id);
        if !folder.is_dir() && record.is_none() {
            return Err(ManageError::Unknown(id.clone()));
        }
        if record.is_some_and(|record| record.origin == Origin::Builtin) {
            return Err(ManageError::Builtin(id.clone()));
        }

        // A disabled record keeps the plugin from starting again while its
        // folder goes, even in a development folder.
        self.managed
            .update(id, None, |record| record.enabled = false)
            .map_err(ManageError::io("cannot save the plugin's record"))?;
        self.refresh(id);

        if folder.exists() {
            std::fs::remove_dir_all(&folder).map_err(ManageError::io(format!(
                "cannot delete {}",
                folder.display()
            )))?;
        }
        // The plugin has stopped, so nothing writes its data again.
        let data = self.dir.join(DATA_DIR).join(format!("{id}.json"));
        match std::fs::remove_file(&data) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(ManageError::io(format!("cannot delete {}", data.display()))(error));
            }
            _ => {}
        }
        self.managed
            .forget(id)
            .map_err(ManageError::io("cannot save the plugin's record"))?;
        drop(managing);
        self.refresh(id);

        Ok(())
    }

    /// Checks a prepared folder (inside the plugins folder, so a move is one rename)
    /// and says what installing it would do.
    pub fn preview_install(&self, staged: &Path, origin: &Origin) -> Result<Preview, ManageError> {
        self.managed.preview_install(&self.dir, staged, origin)
    }

    /// What allowing an installed plugin would grant.
    pub fn preview_allow(&self, id: &PluginId) -> Result<Preview, ManageError> {
        self.managed.preview_allow(&self.dir, id)
    }

    /// Moves a prepared folder in as a new plugin, or as the update of one from the
    /// same source, and records the permissions the user allowed in the preview.
    pub fn install(&self, staged: &Path, origin: Origin) -> Result<PluginId, ManageError> {
        let managing = locked(&self.managing);
        let id = self.managed.install(staged, origin)?;
        drop(managing);
        self.refresh(&id);

        Ok(id)
    }

    fn installed_manifest(&self, id: &PluginId) -> Result<Manifest, ManageError> {
        let folder = self.dir.join(id.to_string());
        if !folder.is_dir() {
            return Err(ManageError::Unknown(id.clone()));
        }

        Ok(Manifest::read(&folder)?)
    }

    /// Has the supervisor read the plugin's folder and record again, and waits for it.
    /// Call it after writing files into a plugin's folder.
    pub fn refresh(&self, id: &PluginId) {
        let (done, finished) = mpsc::channel();
        let sent = self.events.send(Event::Refresh {
            id: id.clone(),
            done,
        });
        if sent.is_ok() {
            let _ = finished.recv();
        }
    }
}

impl Host {
    /// Tells the plugins which slots are on the page now and with what context.
    /// The page sends what changed since its last batch.
    pub fn set_instances(&self, batch: InstanceBatch) -> Result<(), InstanceError> {
        batch.check()?;
        let _ = self.events.send(Event::Instances(batch));

        Ok(())
    }

    /// Hands a press, switch or text change to the plugin that drew the control.
    /// The plugin's thread runs the callback, then draws again. An event for a
    /// control that is no longer shown is ignored, and a plugin gets at most
    /// thirty events a second.
    pub fn event(&self, event: UiEvent) -> Result<(), EventError> {
        event.action.check()?;
        let _ = self.events.send(Event::Ui(event));

        Ok(())
    }
}

impl Host {
    /// Calls a tool a plugin provides, on the plugin's thread, and returns its
    /// result. The plugin must be running (a plugin that waits for its slot starts
    /// for the call) and the arguments must fit the tool's schema. A promise
    /// that does not settle within ten seconds fails the call, not the plugin.
    /// Blocks until the plugin answers.
    pub fn call_tool(
        &self,
        id: &PluginId,
        tool: &ToolName,
        input: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolError> {
        let status = self
            .status()
            .into_iter()
            .find(|status| &status.id == id)
            .ok_or_else(|| ToolError::NoPlugin(id.clone()))?;
        if !matches!(status.state, PluginState::Waiting | PluginState::Running) {
            return Err(ToolError::NotRunning {
                plugin: id.clone(),
                state: status.state,
            });
        }

        let Some(spec) = status.info.tools.iter().find(|spec| &spec.name == tool) else {
            return Err(ToolError::NoTool {
                plugin: id.clone(),
                tool: tool.clone(),
                provided: status
                    .info
                    .tools
                    .iter()
                    .map(|spec| spec.name.clone())
                    .collect(),
            });
        };
        spec.check_input(input)?;
        let text = input.to_string();
        if text.len() > MAX_JSON_BYTES {
            return Err(ToolError::Input(format!(
                "over {MAX_JSON_BYTES} bytes of JSON"
            )));
        }

        let (reply, answer) = mpsc::channel();
        let call = ToolCall {
            name: tool.clone(),
            input: text,
            reply,
        };
        self.events
            .send(Event::Tool {
                id: id.clone(),
                call,
            })
            .map_err(|_| ToolError::Stopped)?;

        // The plugin's own deadline is first; this one covers a thread that is gone.
        let answered =
            answer
                .recv_timeout(TIMEOUT + CALL_GRACE)
                .map_err(|error| match error {
                    RecvTimeoutError::Timeout => ToolError::TimedOut,
                    RecvTimeoutError::Disconnected => ToolError::Stopped,
                })??;

        serde_json::from_str(&answered).map_err(|error| ToolError::Failed(error.to_string()))
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
    mailbox: Arc<Mailbox>,
    limiter: Limiter,
    thread: JoinHandle<()>,
    generation: u64,
    hidden: Arc<AtomicBool>,
}

impl Running {
    fn send(&self, delivery: Delivery) {
        self.inbox.push(delivery);
        self.wake();
    }

    /// Wakes the thread for new work. A hidden plugin keeps it until it shows.
    fn wake(&self) {
        if !self.hidden.load(Ordering::Acquire) {
            self.resume();
        }
    }

    fn resume(&self) {
        // A full channel means a wake is already waiting.
        let _ = self.wake.try_send(());
    }
}

/// What one plugin follows, as its current run reported it.
struct Wanted {
    generation: u64,
    topics: Vec<Topic>,
}

/// Whether something the plugin is activated by has happened.
fn triggered(
    activation: &[Activation],
    instances: &Instances,
    local: &HashMap<Topic, Delivery>,
) -> bool {
    activation.iter().any(|activation| match activation {
        Activation::Startup => true,
        Activation::Slot(slot) => instances.has(*slot),
        Activation::Topic(topic) => local.keys().any(|seen| seen.name == topic.name),
    })
}

/// Whether the plugin stops when its slots are gone: only plugins activated
/// by slots alone do.
fn idles(activation: &[Activation]) -> bool {
    activation
        .iter()
        .all(|activation| matches!(activation, Activation::Slot(_)))
}

fn holds(activation: &[Activation], instances: &Instances) -> bool {
    activation.iter().any(|activation| match activation {
        Activation::Slot(slot) => instances.has(*slot),
        Activation::Startup | Activation::Topic(_) => true,
    })
}

/// What a plugin's last render put on the page.
struct Shown {
    placements: Vec<Placement>,
    assets: Vec<Asset>,
}

/// Owns the running plugins. It is the only caller of the handler.
struct Supervisor {
    dir: PathBuf,
    managed: Arc<Managed>,
    events: Sender<Event>,
    running: HashMap<PluginId, Running>,
    /// What each plugin is activated by, from its manifest.
    plans: HashMap<PluginId, Vec<Activation>>,
    shown: BTreeMap<PluginId, Shown>,
    reloads: HashMap<PluginId, Instant>,
    published: Arc<Published>,
    wanted: BTreeMap<PluginId, Wanted>,
    /// The latest update of each `ui.*` topic.
    local: HashMap<Topic, Delivery>,
    /// The slots on the page now.
    instances: Instances,
    /// The windows that are visible now.
    windows: BTreeSet<String>,
    handler: Shared,
    generation: u64,
    /// The statuses last reported to the handler, without their costs.
    reported: Vec<PluginStatus>,
}

impl Supervisor {
    fn new(
        dir: PathBuf,
        managed: Arc<Managed>,
        events: Sender<Event>,
        handler: Shared,
        published: Arc<Published>,
    ) -> Self {
        Self {
            dir,
            managed,
            events,
            running: HashMap::new(),
            plans: HashMap::new(),
            shown: BTreeMap::new(),
            reloads: HashMap::new(),
            published,
            wanted: BTreeMap::new(),
            local: HashMap::new(),
            instances: Instances::default(),
            windows: BTreeSet::new(),
            handler,
            generation: 0,
            reported: Vec::new(),
        }
    }

    fn run(mut self, inbox: &Receiver<Event>) {
        loop {
            let deadline = [self.next_reload(), self.next_idle_stop()]
                .into_iter()
                .flatten()
                .min();
            let event = match deadline {
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

            let now = Instant::now();
            if let Some(event) = event
                && !self.handle(event)
            {
                break;
            }

            self.reload_due(now);
            self.reconcile(now);
            self.report_status();
        }

        let ids: Vec<PluginId> = self.running.keys().cloned().collect();
        for id in ids {
            self.stop_plugin(&id);
        }
    }

    /// Applies one event. Returns false for `Stop`.
    fn handle(&mut self, event: Event) -> bool {
        match event {
            Event::Stop => return false,
            Event::Fs(path) => self.note_change(&path),
            Event::Refresh { id, done } => {
                self.reloads.remove(&id);
                self.reload(id, Instant::now());
                let _ = done.send(());
            }
            Event::Shown {
                id,
                generation,
                placements,
                assets,
            } => self.note_shown(id, generation, placements, assets),
            Event::Wanted {
                id,
                generation,
                topics,
            } => self.note_wanted(id, generation, topics),
            Event::Stopped {
                id,
                generation,
                reason,
            } => self.note_stopped(&id, generation, reason),
            Event::Publish(delivery) => self.route(delivery),
            Event::Instances(batch) => self.note_instances(batch),
            Event::Visible { window, visible } => self.note_visible(window, visible),
            Event::Ui(event) => self.route_ui(event),
            Event::Tool { id, call } => self.route_tool(&id, call),
        }

        true
    }

    /// Tells the handler when the list of plugins or a state changed.
    fn report_status(&mut self) {
        let now = locked(&self.published.registry).list();
        if now != self.reported {
            self.reported = now;
            self.handler.status_changed();
        }
    }

    fn next_reload(&self) -> Option<Instant> {
        self.reloads.values().min().copied()
    }

    /// When the first plugin that lost its slots is due to stop.
    fn next_idle_stop(&self) -> Option<Instant> {
        locked(&self.published.registry).next_idle_stop()
    }

    fn load_all(&mut self, now: Instant) {
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
                self.register(id, now);
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

    fn note_shown(
        &mut self,
        id: PluginId,
        generation: u64,
        placements: Vec<Placement>,
        assets: Vec<Asset>,
    ) {
        let current = self.running.get(&id).map(|running| running.generation);
        if current != Some(generation) {
            return;
        }

        self.shown.insert(id, Shown { placements, assets });
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

    /// A plugin that failed or went over budget shows and follows nothing.
    fn note_stopped(&mut self, id: &PluginId, generation: u64, reason: String) {
        let current = self.running.get(id).map(|running| running.generation);
        if current != Some(generation) {
            return;
        }

        locked(&self.published.registry).move_to(id, Lifecycle::Stopped(reason));
        self.stop_plugin(id);
        self.clear(id);
    }

    /// Drops what a plugin showed and followed.
    fn clear(&mut self, id: &PluginId) {
        if self.shown.remove(id).is_some() {
            self.publish();
        }
        if self.wanted.remove(id).is_some() {
            self.sync_wanted();
        }
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

    /// Gives every running plugin the instances on the page when they changed.
    fn note_instances(&mut self, batch: InstanceBatch) {
        if !self.instances.apply(batch) {
            return;
        }

        let list = self.instances.list();
        for running in self.running.values() {
            running.mailbox.set_instances(Arc::clone(&list));
            running.wake();
        }
    }

    /// Pauses every plugin when the last visible window hides, and resumes them
    /// when one shows.
    fn note_visible(&mut self, window: String, visible: bool) {
        let before = !self.windows.is_empty();
        if visible {
            self.windows.insert(window);
        } else {
            self.windows.remove(&window);
        }
        let now = !self.windows.is_empty();
        if now == before {
            return;
        }

        locked(&self.published.registry).set_visible(now);
        for running in self.running.values() {
            running.hidden.store(!now, Ordering::Release);
            if now {
                running.resume();
            }
        }
    }

    /// Gives a tool call to the plugin's thread. A plugin that waits for its slot
    /// starts for the call, and one that is off, paused or stopped answers with
    /// its state.
    fn route_tool(&mut self, id: &PluginId, call: ToolCall) {
        let state = locked(&self.published.registry).state(id);
        if state == Some(PluginState::Waiting) && !self.windows.is_empty() {
            self.start_plugin(id);
        }

        let state = locked(&self.published.registry).state(id);
        let refused = match (state, self.running.get(id)) {
            (Some(PluginState::Running), Some(running)) => match running.mailbox.push_call(call) {
                Ok(()) => return running.wake(),
                Err(call) => (call, ToolError::Busy),
            },
            (Some(state), _) => (
                call,
                ToolError::NotRunning {
                    plugin: id.clone(),
                    state,
                },
            ),
            (None, _) => (call, ToolError::NoPlugin(id.clone())),
        };

        let _ = refused.0.reply.send(Err(refused.1));
    }

    /// Gives an event to the plugin that drew the control, within its rate limit.
    fn route_ui(&mut self, event: UiEvent) {
        let Some(running) = self.running.get_mut(&event.plugin) else {
            return;
        };
        if !running.limiter.allow(Instant::now()) {
            debug!("dropped an event for {}: too many a second", event.plugin);
            return;
        }

        running.mailbox.push(event);
        running.wake();
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
            let mut reported = locked(&self.published.wanted);
            if *reported == all {
                return;
            }
            reported.clone_from(&all);
        }

        self.handler.wanted(&all);
    }

    fn reload_due(&mut self, now: Instant) {
        let due: Vec<PluginId> = self
            .reloads
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(id, _)| id.clone())
            .collect();

        for id in due {
            self.reloads.remove(&id);
            self.reload(id, now);
        }
    }

    /// Reads a plugin's folder and record again; a plugin whose folder is gone is forgotten.
    fn reload(&mut self, id: PluginId, now: Instant) {
        if self.dir.join(id.to_string()).is_dir() {
            self.register(id, now);
        } else {
            self.stop_plugin(&id);
            self.plans.remove(&id);
            locked(&self.published.registry).remove(&id);
            self.clear(&id);
        }
    }

    /// Reads a plugin's manifest and record again, then starts it if it may run and
    /// is activated. What it showed stays until its new run replaces it.
    fn register(&mut self, id: PluginId, now: Instant) {
        self.stop_plugin(&id);

        match Manifest::read(&self.dir.join(id.to_string())) {
            Ok(manifest) => {
                let assessed = self.managed.assess(&id, &manifest);
                let lifecycle = match assessed.verdict {
                    Verdict::Run => Lifecycle::Waiting,
                    Verdict::Disabled => Lifecycle::Disabled,
                    Verdict::NeedsApproval(missing) => Lifecycle::NeedsApproval(missing),
                };
                self.plans.insert(id.clone(), manifest.activation);
                locked(&self.published.registry).set(
                    id.clone(),
                    Info {
                        name: manifest.name,
                        version: manifest.version,
                        description: manifest.description,
                        permissions: manifest.permissions,
                        approved: assessed.approved,
                        source: assessed.origin,
                        tools: manifest.tools,
                        style: manifest.style,
                        enabled: assessed.enabled,
                    },
                    lifecycle,
                );
                self.reconcile_one(&id, now);

                let current = locked(&self.published.registry).lifecycle(&id).cloned();
                if !matches!(
                    current,
                    Some(Lifecycle::Running | Lifecycle::IdleGrace(_) | Lifecycle::Stopped(_))
                        | None
                ) {
                    self.clear(&id);
                }
            }
            Err(error) => {
                emit(&id, Level::Error, &format!("stopped: {error}"));
                self.plans.remove(&id);
                let record = self.managed.record(&id);
                locked(&self.published.registry).set(
                    id.clone(),
                    Info {
                        name: id.to_string(),
                        version: semver::Version::new(0, 0, 0),
                        description: String::new(),
                        permissions: Vec::new(),
                        approved: record
                            .as_ref()
                            .map(|record| record.approved.clone())
                            .unwrap_or_default(),
                        source: record
                            .as_ref()
                            .map_or(Origin::Local, |record| record.origin.clone()),
                        tools: Vec::new(),
                        style: Style::new(),
                        enabled: record.is_none_or(|record| record.enabled),
                    },
                    Lifecycle::Stopped(error.to_string()),
                );
                self.clear(&id);
            }
        }
    }

    /// Starts the waiting plugins whose activation holds, and stops the running
    /// ones whose slots have been off the page for the grace period.
    fn reconcile(&mut self, now: Instant) {
        let ids: Vec<PluginId> = self.plans.keys().cloned().collect();
        for id in ids {
            self.reconcile_one(&id, now);
        }
    }

    fn reconcile_one(&mut self, id: &PluginId, now: Instant) {
        let Some(activation) = self.plans.get(id) else {
            return;
        };
        let lifecycle = locked(&self.published.registry).lifecycle(id).cloned();

        match lifecycle {
            Some(Lifecycle::Waiting) => {
                if triggered(activation, &self.instances, &self.local) {
                    self.start_plugin(id);
                }
            }
            Some(Lifecycle::Running)
                if idles(activation) && !holds(activation, &self.instances) =>
            {
                locked(&self.published.registry)
                    .move_to(id, Lifecycle::IdleGrace(now + IDLE_GRACE));
            }
            Some(Lifecycle::IdleGrace(_)) if holds(activation, &self.instances) => {
                locked(&self.published.registry).move_to(id, Lifecycle::Running);
            }
            Some(Lifecycle::IdleGrace(deadline)) if deadline <= now => {
                self.stop_plugin(id);
                locked(&self.published.registry).move_to(id, Lifecycle::Waiting);
                self.clear(id);
            }
            Some(
                Lifecycle::Running
                | Lifecycle::IdleGrace(_)
                | Lifecycle::Stopped(_)
                | Lifecycle::Disabled
                | Lifecycle::NeedsApproval(_),
            )
            | None => {}
        }
    }

    fn start_plugin(&mut self, id: &PluginId) {
        self.stop_plugin(id);
        self.generation += 1;
        let generation = self.generation;
        let (wake, woken) = mpsc::sync_channel(1);
        let inbox = Arc::new(Inbox::default());
        let mailbox = Arc::new(Mailbox::default());
        mailbox.set_instances(self.instances.list());
        let meter = Arc::new(Meter::new(BUDGET));
        let hidden = Arc::new(AtomicBool::new(self.windows.is_empty()));
        let job = PluginJob {
            dir: self.dir.join(id.to_string()),
            id: id.clone(),
            generation,
            events: self.events.clone(),
            inbox: Arc::clone(&inbox),
            mailbox: Arc::clone(&mailbox),
            meter: Arc::clone(&meter),
            hidden: Arc::clone(&hidden),
        };

        let spawned = thread::Builder::new()
            .name(format!("plugin-{id}"))
            .spawn(move || job.run(&woken));
        let mut registry = locked(&self.published.registry);
        match spawned {
            Ok(thread) => {
                registry.move_to(id, Lifecycle::Running);
                self.running.insert(
                    id.clone(),
                    Running {
                        wake,
                        inbox,
                        mailbox,
                        limiter: Limiter::default(),
                        thread,
                        generation,
                        hidden,
                    },
                );
            }
            Err(error) => {
                emit(id, Level::Error, &format!("cannot start: {error}"));
                registry.move_to(id, Lifecycle::Stopped(format!("cannot start: {error}")));
                drop(registry);
                self.clear(id);
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
        let all: Vec<Placement> = self
            .shown
            .values()
            .flat_map(|shown| shown.placements.iter().cloned())
            .collect();
        // The page asks for an image as soon as it hears of a placement.
        *locked(&self.published.assets) = self
            .shown
            .values()
            .flat_map(|shown| &shown.assets)
            .map(|asset| (asset.hash().clone(), asset.clone()))
            .collect();
        {
            let mut published = locked(&self.published.shown);
            if *published == all {
                return;
            }
            published.clone_from(&all);
        }

        self.handler.shown(&all);
    }
}

#[cfg(test)]
#[path = "tests/host.rs"]
mod tests;
