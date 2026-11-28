//! Topic streams own the sources they keep running. Registries hold weak references only.

use super::roots::{
    FileBindings, FileWatcher, MAX_FILE_ROOTS, TOO_MANY_ROOTS, broad_files, has_git, resolve,
};
use crate::link::Offers;
use crate::{
    AsyncMutex,
    events::{Change, ChangeKind, Touched},
};
use anyhow::{Context as _, Result, ensure};
use beep_proto::node::events::Topic;
use futures::{
    Stream, StreamExt,
    future::{join_all, try_join_all},
    stream,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::{sync::broadcast, task::JoinSet, time::Instant};
use tokio_stream::{
    StreamMap, wrappers::BroadcastStream, wrappers::errors::BroadcastStreamRecvError,
};

/// How often a changing root without Git metadata asks Git again. Each try
/// binds every root of the source again, so a busy folder outside any
/// repository must not make it run often.
const GIT_RETRY: Duration = Duration::from_secs(30);

pub(crate) type Registry = AsyncMutex<BTreeMap<Topic, Weak<Source>>>;
pub(super) type Subscriptions = StreamMap<Topic, Subscription>;

pub(crate) struct Source {
    topic: Topic,
    sender: broadcast::Sender<Change>,
    /// What the topic resolves to now. Published as soon as it is known,
    /// before the watcher binds it, so a snapshot never waits for the watcher.
    roots: Mutex<BTreeMap<Topic, PathBuf>>,
    /// Tickets for `resolve`: taken before a lookup starts, and the newest
    /// one whose answer was stored, so an older lookup never lands over it.
    issued: AtomicU64,
    stored: AtomicU64,
    /// Topics whose root came or went, announced once the watcher has bound.
    unannounced: Mutex<BTreeSet<Topic>>,
    /// Held only to read or swap the watcher.
    watcher: Mutex<Option<Arc<FileWatcher>>>,
    /// Held across the walk that installs a binding, so installs of one
    /// source stay in order. Nothing else waits on it.
    install: AsyncMutex<()>,
    /// When a root bound without Git metadata last asked Git again.
    tried: Mutex<Option<Instant>>,
    /// For a test: the next lookup says it is done, then waits to be released.
    #[cfg(test)]
    after_lookup: Mutex<Option<Gate>>,
    _agent_sources: Option<Arc<crate::agents::follow::Sources>>,
    _processes: Option<crate::agents::ProcessDemand>,
    _devices: Option<crate::run::device::Follow>,
}

impl Source {
    async fn new(offers: &Offers, topic: &Topic) -> Result<Self> {
        let agent_sources = if broad_files(std::slice::from_ref(topic))
            || matches!(topic.name(), "agent" | "agents" | "agent-log" | "*")
        {
            Some(offers.agents.bridge.watch_event_sources().await?)
        } else {
            None
        };
        let source = Self {
            topic: topic.clone(),
            sender: broadcast::channel(1024).0,
            roots: Mutex::default(),
            issued: AtomicU64::new(0),
            stored: AtomicU64::new(0),
            unannounced: Mutex::default(),
            watcher: Mutex::default(),
            install: AsyncMutex::default(),
            tried: Mutex::default(),
            #[cfg(test)]
            after_lookup: Mutex::default(),
            _agent_sources: agent_sources,
            _processes: matches!(topic.name(), "processes" | "*")
                .then(crate::agents::ProcessDemand::new),
            _devices: matches!(
                topic.name(),
                crate::run::device::LIST_TOPIC | crate::run::device::TOPIC
            )
            .then(crate::run::device::Follow::new),
        };
        source.resolve(offers).await?;
        // Nobody else watches it yet. The stream that made it reads again
        // once it is bound.
        crate::lock(&source.unannounced).clear();
        Ok(source)
    }

    /// Looks up the roots the topic names now and publishes them. Takes no
    /// lock across the lookups. Of two overlapping lookups the one that
    /// started last wins, whichever finishes last.
    async fn resolve(&self, offers: &Offers) -> Result<()> {
        let ticket = self.issued.fetch_add(1, Ordering::Relaxed) + 1;
        let wanted = resolve(offers, std::slice::from_ref(&self.topic)).await?;
        #[cfg(test)]
        {
            let gate = crate::lock(&self.after_lookup).take();

            if let Some(gate) = gate {
                let _ = gate.reached.send(());
                let _ = gate.release.await;
            }
        }

        let mut roots = crate::lock(&self.roots);

        if ticket < self.stored.load(Ordering::Relaxed) {
            return Ok(());
        }

        self.stored.store(ticket, Ordering::Relaxed);
        let previous = std::mem::replace(&mut *roots, wanted.clone());
        drop(roots);

        // A root can appear or disappear while a broad subscription remains
        // active. The watcher announces it once it is bound, which covers
        // the gap before it.
        if previous != wanted {
            crate::lock(&self.unannounced).extend(previous.into_keys().chain(wanted.into_keys()));
        }

        Ok(())
    }

    /// Brings the file watcher in line with the published roots. Git is
    /// asked outside the lock, which is taken to compare and swap.
    async fn bind(&self, offers: &Offers) -> Result<()> {
        let wanted = crate::lock(&self.roots).clone();
        let current = held(crate::lock(&self.watcher).as_deref());
        let bindings = git_bindings(&wanted, &current).await;
        #[cfg(test)]
        {
            let gate = {
                let mut gates = crate::lock(&offers.bind_gates);
                gates
                    .iter()
                    .position(|gate| wanted.values().any(|root| *root == gate.root))
                    .map(|at| gates.remove(at))
            };

            if let Some(gate) = gate {
                let _ = gate.reached.send(());
                let _ = gate.release.await;
            }
        }

        let _install = self.install.lock().await;

        // A newer resolve is followed by its own bind.
        if *crate::lock(&self.roots) != wanted {
            return Ok(());
        }

        let watcher = {
            let mut watcher = crate::lock(&self.watcher);

            if wanted.is_empty() {
                *watcher = None;
                None
            } else if bindings != held(watcher.as_deref()) {
                if watcher.is_none() {
                    *watcher = Some(Arc::new(file_watcher(offers)?));
                }

                watcher.clone()
            } else {
                None
            }
        };

        if let Some(watcher) = watcher {
            let paths = bindings.values().flatten().cloned().collect();
            watcher.paths.replace(paths).await?;
            *crate::lock(&watcher.bindings) = bindings.clone();
        }

        for topic in std::mem::take(&mut *crate::lock(&self.unannounced)) {
            offers.agents.events.changed(topic);
        }

        Ok(())
    }

    /// Whether a root was bound without Git metadata.
    fn has_bare(&self) -> bool {
        held(crate::lock(&self.watcher).as_deref())
            .values()
            .any(|dependencies| !has_git(dependencies))
    }

    /// Whether one of the `changed` roots was bound without Git metadata and
    /// should ask Git again; at most once per `GIT_RETRY`.
    fn retry_git<'a>(&self, mut changed: impl Iterator<Item = &'a Topic>) -> bool {
        let bindings = held(crate::lock(&self.watcher).as_deref());
        let mut tried = crate::lock(&self.tried);

        if !changed.any(|topic| bindings.get(topic).is_some_and(|d| !has_git(d)))
            || tried.is_some_and(|tried| tried.elapsed() < GIT_RETRY)
        {
            return false;
        }

        *tried = Some(Instant::now());
        true
    }
}

fn held(watcher: Option<&FileWatcher>) -> FileBindings {
    watcher
        .map(|watcher| crate::lock(&watcher.bindings).clone())
        .unwrap_or_default()
}

/// The watcher for one source: its callback turns a change under a bound
/// root into an event for that root's topic.
fn file_watcher(offers: &Offers) -> Result<FileWatcher> {
    let bindings = Arc::new(Mutex::new(FileBindings::new()));
    let changed_roots = bindings.clone();
    let hub = offers.agents.events.clone();
    let paths = crate::events::paths::Paths::with_paths("link-events", move |paths, _, moved| {
        let held = crate::lock(&changed_roots);

        for (topic, dependencies) in held.iter() {
            // The root comes first; the rest are its Git metadata.
            let Some((root, git)) = dependencies.split_first() else {
                continue;
            };

            if git.iter().any(|dependency| paths.contains(dependency)) {
                let touched = touched_of(root, git, moved.as_deref());
                hub.changed_touching(topic.clone(), ChangeKind::Any, touched);
            } else if paths.contains(root) {
                let touched = touched_of(root, git, moved.as_deref());
                hub.changed_touching(topic.clone(), ChangeKind::WorkTree, touched);
            }
        }
    })?;

    Ok(FileWatcher { paths, bindings })
}

/// Which paths of `root` the `moved` paths touch: those under it by their path in it, and a
/// change in its Git metadata outside it as `.git`. Unknown when `moved` is, or when the root
/// itself or a folder above it changed.
fn touched_of(root: &Path, git: &[PathBuf], moved: Option<&[PathBuf]>) -> Touched {
    let Some(moved) = moved else {
        return Touched::All;
    };
    let mut named = Vec::new();

    for path in moved {
        if let Ok(inside) = path.strip_prefix(root) {
            if inside.as_os_str().is_empty() {
                return Touched::All;
            }

            let parts: Vec<_> = inside
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect();
            named.push(parts.join("/"));
        } else if git.iter().any(|dependency| path.starts_with(dependency)) {
            named.push(".git".to_owned());
        } else if root.starts_with(path) {
            return Touched::All;
        }
    }

    Touched::of(named)
}

/// Each root with the Git metadata to watch beside it. A root kept from the
/// last pass keeps its Git paths without asking Git again. One without them
/// asks again: it may have become a repository, or Git failed.
async fn git_bindings(wanted: &BTreeMap<Topic, PathBuf>, held: &FileBindings) -> FileBindings {
    stream::iter(wanted.clone())
        .map(|(topic, root)| {
            let kept = held
                .get(&topic)
                .filter(|dependencies| dependencies.first() == Some(&root) && has_git(dependencies))
                .cloned();
            async move {
                if let Some(dependencies) = kept {
                    return (topic, dependencies);
                }

                let mut dependencies = vec![root.clone()];

                match crate::agents::checkouts::watch_paths(&root).await {
                    Ok(paths) => dependencies.extend(paths),
                    Err(error) => {
                        log::debug!("Git metadata notifications unavailable: {error:#}")
                    }
                }
                (topic, dependencies)
            }
        })
        .buffer_unordered(8)
        .collect()
        .await
}

/// Holds a bind or a lookup, for a test: it says it is there, then waits.
#[cfg(test)]
pub(crate) struct Gate {
    pub(crate) root: PathBuf,
    pub(crate) reached: tokio::sync::oneshot::Sender<()>,
    pub(crate) release: tokio::sync::oneshot::Receiver<()>,
}

pub(super) struct Subscription {
    stream: BroadcastStream<Change>,
    pub(super) source: Arc<Source>,
}

impl Stream for Subscription {
    type Item = Result<Change, BroadcastStreamRecvError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.stream).poll_next(cx)
    }
}

pub(super) async fn subscribe(offers: &Offers, topics: &[Topic]) -> Result<Subscriptions> {
    ensure!(
        topics.iter().all(Topic::valid),
        "invalid subscription topics"
    );
    // The topics are looked up together: each may wait on a worktree list or a mesh lookup.
    let found = join_all(topics.iter().map(|topic| source_of(offers, topic)))
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    let mut subscriptions = Subscriptions::new();

    for (topic, source) in topics.iter().zip(found) {
        let stream = BroadcastStream::new(source.sender.subscribe());
        subscriptions.insert(topic.clone(), Subscription { stream, source });
    }

    ensure!(
        roots(&subscriptions).len() <= MAX_FILE_ROOTS,
        TOO_MANY_ROOTS
    );

    Ok(subscriptions)
}

/// The running source of `topic`, started when no stream holds one, with its roots looked up now.
async fn source_of(offers: &Offers, topic: &Topic) -> Result<Arc<Source>> {
    let source = {
        let mut registry = offers.event_sources.lock().await;
        registry.retain(|_, source| source.strong_count() > 0);
        registry.get(topic).and_then(Weak::upgrade)
    };

    let (source, reused) = match source {
        Some(source) => (source, true),
        None => {
            let candidate = Arc::new(Source::new(offers, topic).await?);
            let mut registry = offers.event_sources.lock().await;

            match registry.get(topic).and_then(Weak::upgrade) {
                Some(source) => (source, true),
                None => {
                    offers
                        .agents
                        .events
                        .register(topic.clone(), &candidate.sender);
                    registry.insert(topic.clone(), Arc::downgrade(&candidate));
                    (candidate, false)
                }
            }
        }
    };

    if reused {
        // A reused source still checks the node's current allowed roots.
        source.resolve(offers).await?;
    }

    Ok(source)
}

pub(super) fn roots(subscriptions: &Subscriptions) -> BTreeMap<Topic, PathBuf> {
    source_roots(&held_sources(subscriptions))
}

/// The sources a stream holds, for a pass to run on while the stream goes on.
fn held_sources(subscriptions: &Subscriptions) -> Vec<Arc<Source>> {
    subscriptions
        .values()
        .map(|subscription| subscription.source.clone())
        .collect()
}

/// One pass over some sources: look their roots up again when `resolve`,
/// then bind their watchers. The sources of a pass run together.
pub(super) struct Pass {
    sources: Vec<Arc<Source>>,
    resolve: bool,
    /// Bind the roots that came out bare once more after the watcher is
    /// installed: one may have become a repository before it was.
    rebind_bare: bool,
}

impl Pass {
    /// Every source of the stream, roots looked up again.
    pub(super) fn all(subscriptions: &Subscriptions) -> Self {
        Self {
            sources: held_sources(subscriptions),
            resolve: true,
            rebind_bare: false,
        }
    }

    /// Every source of the stream, whose roots are known: the first bind
    /// of a watch, before which no watcher saw a change.
    pub(super) fn bind(subscriptions: &Subscriptions) -> Self {
        Self {
            sources: held_sources(subscriptions),
            resolve: false,
            rebind_bare: true,
        }
    }

    /// The sources that have a changed root bound without Git metadata and
    /// may ask Git again, or none.
    pub(super) fn git<'a>(
        subscriptions: &Subscriptions,
        changed: impl Iterator<Item = &'a Topic> + Clone,
    ) -> Option<Self> {
        let sources: Vec<_> = subscriptions
            .values()
            .filter(|subscription| subscription.source.retry_git(changed.clone()))
            .map(|subscription| subscription.source.clone())
            .collect();

        (!sources.is_empty()).then_some(Self {
            sources,
            resolve: false,
            rebind_bare: false,
        })
    }

    fn merge(&mut self, other: Self) {
        self.resolve |= other.resolve;
        self.rebind_bare |= other.rebind_bare;

        for source in other.sources {
            if !self.sources.iter().any(|held| Arc::ptr_eq(held, &source)) {
                self.sources.push(source);
            }
        }
    }

    async fn run(self, offers: Offers) -> Result<()> {
        if self.resolve {
            try_join_all(self.sources.iter().map(|source| source.resolve(&offers))).await?;
        }

        ensure!(
            source_roots(&self.sources).len() <= MAX_FILE_ROOTS,
            TOO_MANY_ROOTS
        );
        try_join_all(self.sources.iter().map(|source| source.bind(&offers))).await?;

        if self.rebind_bare {
            let bare = self.sources.iter().filter(|source| source.has_bare());
            try_join_all(bare.map(|source| source.bind(&offers))).await?;
        }

        Ok(())
    }
}

/// The passes of one stream, off its loop: one runs, and what asks for
/// another meanwhile is run together after it.
pub(super) struct Passes {
    offers: Offers,
    running: JoinSet<Result<()>>,
    next: Option<Pass>,
}

impl Passes {
    pub(super) fn new(offers: &Offers) -> Self {
        Self {
            offers: offers.clone(),
            running: JoinSet::new(),
            next: None,
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.running.is_empty()
    }

    pub(super) fn start(&mut self, pass: Pass) {
        if self.running.is_empty() {
            self.running.spawn(pass.run(self.offers.clone()));
        } else if let Some(next) = &mut self.next {
            next.merge(pass);
        } else {
            self.next = Some(pass);
        }
    }

    /// Waits for the running pass, then starts the one that queued behind it.
    pub(super) async fn finished(&mut self) -> Result<()> {
        let Some(joined) = self.running.join_next().await else {
            return std::future::pending().await;
        };

        if let Some(next) = self.next.take() {
            self.start(next);
        }

        joined.context("a topic pass stopped")?
    }
}

fn source_roots(sources: &[Arc<Source>]) -> BTreeMap<Topic, PathBuf> {
    sources
        .iter()
        .flat_map(|source| crate::lock(&source.roots).clone())
        .collect()
}

#[cfg(test)]
#[path = "../../tests/link_events_sources.rs"]
mod tests;
