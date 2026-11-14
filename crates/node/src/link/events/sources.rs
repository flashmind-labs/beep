//! Topic streams own the sources they keep running. Registries hold weak references only.

use super::{FileWatcher, MAX_FILE_ROOTS, Offers, TOO_MANY_ROOTS, broad_files, has_git, watch};
use crate::{AsyncMutex, events::Change};
use anyhow::{Result, ensure};
use beep_proto::node::events::Topic;
use futures::Stream;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    pin::Pin,
    sync::{Arc, Mutex, Weak},
    task::{Context, Poll},
    time::Duration,
};
use tokio::{sync::broadcast, time::Instant};
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
    roots: Arc<Mutex<BTreeMap<Topic, PathBuf>>>,
    watcher: AsyncMutex<Option<FileWatcher>>,
    bare: Mutex<Bare>,
    _agent_sources: Option<Arc<crate::agents::follow::Sources>>,
    _processes: Option<crate::agents::ProcessDemand>,
}

impl Source {
    async fn new(offers: &Offers, topic: &Topic) -> Result<Self> {
        let agent_sources = if broad_files(std::slice::from_ref(topic))
            || matches!(topic.name(), "agent" | "agents" | "*")
        {
            Some(offers.agents.watch_event_sources().await?)
        } else {
            None
        };
        let source = Self {
            topic: topic.clone(),
            sender: broadcast::channel(1024).0,
            roots: Arc::default(),
            watcher: AsyncMutex::default(),
            bare: Mutex::default(),
            _agent_sources: agent_sources,
            _processes: matches!(topic.name(), "processes" | "*")
                .then(crate::agents::ProcessDemand::new),
        };
        source.reconcile(offers).await?;
        Ok(source)
    }

    pub(super) async fn reconcile(&self, offers: &Offers) -> Result<()> {
        let mut watcher = self.watcher.lock().await;
        let watched = watch(
            offers,
            &mut watcher,
            &self.roots,
            std::slice::from_ref(&self.topic),
        )
        .await;
        let bare = watcher
            .as_ref()
            .map(|watcher| {
                crate::lock(&watcher.bindings)
                    .iter()
                    .filter(|(_, dependencies)| !has_git(dependencies))
                    .map(|(topic, _)| topic.clone())
                    .collect()
            })
            .unwrap_or_default();
        crate::lock(&self.bare).topics = bare;
        watched
    }

    /// Whether one of the `changed` roots was bound without Git metadata and
    /// should ask Git again; at most once per `GIT_RETRY`.
    fn retry_git<'a>(&self, mut changed: impl Iterator<Item = &'a Topic>) -> bool {
        let mut bare = crate::lock(&self.bare);

        if !changed.any(|topic| bare.topics.contains(topic))
            || bare.tried.is_some_and(|tried| tried.elapsed() < GIT_RETRY)
        {
            return false;
        }

        bare.tried = Some(Instant::now());
        true
    }
}

/// The roots a source bound without Git metadata, and when it last asked
/// Git again.
#[derive(Default)]
struct Bare {
    topics: BTreeSet<Topic>,
    tried: Option<Instant>,
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
    let mut subscriptions = Subscriptions::new();

    for topic in topics {
        let source = {
            let mut registry = offers.agents.event_sources.lock().await;
            registry.retain(|_, source| source.strong_count() > 0);
            registry.get(topic).and_then(Weak::upgrade)
        };
        let (source, reused) = match source {
            Some(source) => (source, true),
            None => {
                let candidate = Arc::new(Source::new(offers, topic).await?);
                let mut registry = offers.agents.event_sources.lock().await;

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
            source.reconcile(offers).await?;
        }

        let stream = BroadcastStream::new(source.sender.subscribe());
        subscriptions.insert(topic.clone(), Subscription { stream, source });
        ensure!(
            roots(&subscriptions).len() <= MAX_FILE_ROOTS,
            TOO_MANY_ROOTS
        );
    }

    Ok(subscriptions)
}

pub(super) fn roots(subscriptions: &Subscriptions) -> BTreeMap<Topic, PathBuf> {
    subscriptions
        .values()
        .flat_map(|subscription| crate::lock(&subscription.source.roots).clone())
        .collect()
}

pub(super) async fn reconcile(offers: &Offers, subscriptions: &Subscriptions) -> Result<()> {
    for subscription in subscriptions.values() {
        subscription.source.reconcile(offers).await?;
    }

    ensure!(
        roots(subscriptions).len() <= MAX_FILE_ROOTS,
        TOO_MANY_ROOTS
    );
    Ok(())
}

/// Asks Git again for every changed root a source bound without Git
/// metadata: it may have become a repository.
pub(super) async fn retry_git<'a>(
    offers: &Offers,
    subscriptions: &Subscriptions,
    changed: impl Iterator<Item = &'a Topic> + Clone,
) -> Result<()> {
    for subscription in subscriptions.values() {
        if subscription.source.retry_git(changed.clone()) {
            subscription.source.reconcile(offers).await?;
        }
    }

    Ok(())
}
