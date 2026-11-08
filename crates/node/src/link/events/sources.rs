//! Topic streams own the sources they keep running. Registries hold weak references only.

use super::{FileWatcher, MAX_FILE_ROOTS, Offers, broad_files, watch};
use crate::{AsyncMutex, events::Change};
use anyhow::{Result, ensure};
use beep_proto::node::events::Topic;
use futures::Stream;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    pin::Pin,
    sync::{Arc, Mutex, Weak},
    task::{Context, Poll},
};
use tokio::sync::broadcast;
use tokio_stream::{
    StreamMap, wrappers::BroadcastStream, wrappers::errors::BroadcastStreamRecvError,
};

pub(crate) type Registry = AsyncMutex<BTreeMap<Topic, Weak<Source>>>;
pub(super) type Subscriptions = StreamMap<Topic, Subscription>;

pub(crate) struct Source {
    topic: Topic,
    sender: broadcast::Sender<Change>,
    roots: Arc<Mutex<BTreeMap<Topic, PathBuf>>>,
    watcher: AsyncMutex<Option<FileWatcher>>,
    _agent_sources: Option<Arc<crate::agents::follow::Sources>>,
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
            _agent_sources: agent_sources,
        };
        source.reconcile(offers).await?;
        Ok(source)
    }

    async fn reconcile(&self, offers: &Offers) -> Result<()> {
        let mut watcher = self.watcher.lock().await;
        watch(
            offers,
            &mut watcher,
            &self.roots,
            std::slice::from_ref(&self.topic),
        )
        .await
    }
}

pub(super) struct Subscription {
    stream: BroadcastStream<Change>,
    source: Arc<Source>,
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
            "too many watched directories; subscribe to specific file roots"
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
        "too many watched directories; subscribe to specific file roots"
    );
    Ok(())
}
