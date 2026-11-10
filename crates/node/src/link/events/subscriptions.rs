//! Topic snapshots on the subscription protocol.

use super::*;
use beep_proto::node::subscriptions::{Reply, Request};

pub(crate) async fn serve(
    mut tx: LinkTx,
    mut rx: LinkRx,
    offers: Offers,
    caller: iroh::EndpointId,
) -> Result<()> {
    let mut snapshots = Snapshots::new();
    let stream = snapshots.stream();
    tracing::info!(%stream, "subscription stream opened");
    // Several controllers may each keep one stream per demanded topic.
    static READERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(256);
    let _reader = READERS.try_acquire().context("too many topic readers")?;
    let mut membership = offers.mesh.subscribe();
    let mut project_changes = membership
        .borrow_and_update()
        .as_ref()
        .map(beep_mesh::Mesh::changes);
    offers.agents.start_event_bridge();
    let mut topics = Vec::new();
    let mut queries = Vec::new();
    let mut subscriptions = Subscriptions::new();
    let mut roots = BTreeMap::new();
    let mut heartbeat = interval_at(
        Instant::now() + Duration::from_secs(10),
        Duration::from_secs(10),
    );
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
    send(&mut tx, Reply::Ready).await?;

    loop {
        snapshots.start(&offers, caller);
        let snapshot_due = snapshots.due();
        tokio::select! {
            _ = async {
                match snapshot_due {
                    Some(due) => tokio::time::sleep_until(due).await,
                    None => std::future::pending().await,
                }
            }, if snapshot_due.is_some() => {},
            request = rx.recv::<Request>() => {
                anyhow::ensure!(offers.lends_to_peers() || offers.mesh.get().is_some_and(|mesh| mesh.me() == caller), "this machine is no longer shared");
                match request? {
                None => return Ok(()),
                Some(Request::Watch { id, topic, queries: wanted_queries }) => {
                    let wanted = vec![topic];
                    let started = Instant::now();
                    tracing::info!(%stream, watch = id, topics = wanted.len(), queries = wanted_queries.len(), "subscription watch received");
                    for query in &wanted_queries {
                        tracing::info!(%stream, watch = id, query = query.id, topic = query.topic.name(), "subscription query received");
                    }

                    let valid = if wanted.len() > events::MAX_TOPICS {
                        Err(anyhow::anyhow!("too many subscription topics"))
                    } else if wanted.iter().any(|topic| !topic.valid()) {
                        Err(anyhow::anyhow!("invalid subscription topics"))
                    } else { Snapshots::validate(&wanted_queries) };
                    let source_topics = sources(&wanted, &wanted_queries);
                    let result = match valid {
                        Ok(()) => sources::subscribe(&offers, &source_topics).await,
                        Err(error) => Err(error),
                    };

                    match result {
                        Ok(next) => {
                            subscriptions = next;
                            roots = sources::roots(&subscriptions);
                            topics = wanted;
                            queries = wanted_queries;
                            snapshots.replace(&offers, &topics, &queries, &roots);
                            send(&mut tx, Reply::Watching { id }).await?;
                            tracing::info!(%stream, watch = id, elapsed_ms = started.elapsed().as_millis() as u64, "subscription watch acknowledged");
                        }
                        Err(error) => {
                            tracing::warn!(%stream, watch = id, elapsed_ms = started.elapsed().as_millis() as u64, "subscription watch rejected");
                            send(&mut tx, Reply::Error { id, reason: format!("{error:#}") }).await?;
                        },
                    }
                }
                Some(Request::Refresh { id }) => {
                    snapshots.reset();
                    send(&mut tx, Reply::Watching { id }).await?;
                },
                Some(Request::Ping { id }) => send(&mut tx, Reply::Pong { id }).await?,

                }
            },
            snapshot = snapshots.next() => if let Some(snapshot) = snapshot? {
                let started = Instant::now();
                let (topic, query, success) = match &snapshot {
                    Reply::Snapshot(value) => (value.topic.name().to_owned(), value.query, true),
                    Reply::SnapshotError { topic, query, .. } => (topic.name().to_owned(), *query, false),
                    _ => unreachable!("snapshot queue only returns snapshots"),
                };
                tracing::debug!(%stream, %topic, query, success, "subscription snapshot sending");
                send(&mut tx, snapshot).await?;
                tracing::debug!(%stream, %topic, query, success, elapsed_ms = started.elapsed().as_millis() as u64, "subscription snapshot sent");
            },
            Some((_, event)) = subscriptions.next(), if !subscriptions.is_empty() => {
                // Overlapping keys can receive the same broad invalidation.
                // Drain a bounded ready batch before reconciling its sources.
                let mut batch = vec![event];

                for _ in 0..subscriptions.len() {
                    let Some(Some((_, event))) = subscriptions.next().now_or_never() else {
                        break;
                    };
                    batch.push(event);
                }

                let mut changed = HashSet::new();
                let mut lagged = false;

                for event in batch {
                    match event {
                        Ok(event) => { changed.insert(event.topic); }
                        Err(BroadcastStreamRecvError::Lagged(_)) => lagged = true,
                    }
                }

                let reconcile = lagged || broad_files(&sources(&topics, &queries))
                    && changed.iter().any(|topic| matches!(topic.name(), "agents" | "projects" | "*"));
                if reconcile {
                    sources::reconcile(&offers, &subscriptions).await?;
                    roots = sources::roots(&subscriptions);
                }
                // Publication can add a previously unknown exact plugin topic.
                if reconcile || changed.iter().any(|topic| !snapshots.contains(topic) && offers.agents.events.current(topic).is_some()) {
                    snapshots.replace(&offers, &topics, &queries, &roots);
                }

                if lagged {
                    snapshots.reset();
                }

                for topic in changed {
                    snapshots.changed(&topic);
                }
            },
            changed = membership.changed(), if watches_files(&sources(&topics, &queries)) => {
                if changed.is_err() { return Ok(()); }
                project_changes = membership.borrow_and_update().as_ref().map(beep_mesh::Mesh::changes);
                sources::reconcile(&offers, &subscriptions).await?;
                roots = sources::roots(&subscriptions);
                snapshots.replace(&offers, &topics, &queries, &roots);
            },
            change = async { match &mut project_changes { Some(changes) => changes.recv().await, None => std::future::pending().await } }, if watches_files(&sources(&topics, &queries)) => {
                let reconcile = match change {
                    Ok(beep_mesh::Key::Project(_) | beep_mesh::Key::Sync(_)) | Err(RecvError::Lagged(_)) => true,
                    Err(RecvError::Closed) => { project_changes = None; false },
                    _ => false,
                };

                if reconcile {
                    sources::reconcile(&offers, &subscriptions).await?;
                    roots = sources::roots(&subscriptions);
                    snapshots.replace(&offers, &topics, &queries, &roots);
                }
            },
            _ = heartbeat.tick() => send(&mut tx, Reply::Pong { id: 0 }).await?,
        }
    }
}

async fn send(tx: &mut LinkTx, reply: Reply) -> Result<()> {
    timeout(WRITE_WAIT, tx.send(&reply))
        .await
        .context("subscription reader stopped receiving")?
}
