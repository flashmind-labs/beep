//! Event sources for sessions on this machine.

use super::{SummaryCache, observers::*, recent_deadline};
use crate::{agents::Agents, logs::found::Agent};
use anyhow::{Context, Result};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::mpsc,
    task::spawn_blocking,
    time::{Instant, sleep},
};

impl Agents {
    pub(in crate::agents) fn observe_external(
        &self,
        sessions: &[beep_proto::node::agent::ExternalSession],
    ) {
        let Some(source) = crate::lock(&self.sources).upgrade() else {
            return;
        };
        let mut processes = crate::lock(&source.processes);
        let before = active_roots(&processes);
        processes.retain(|_, held| !held._task.is_finished());

        for session in sessions {
            let key = (session.agent.clone(), session.id.clone());
            let deadline = recent_deadline(session);
            let revision = match (session.pid, deadline) {
                (Some(pid), _) => Observation::Process(pid),
                (_, Some(_)) => Observation::Recent(session.last_activity),
                _ => {
                    processes.remove(&key);
                    continue;
                }
            };

            if let Some(held) = processes.get_mut(&key)
                && held.revision == revision
            {
                held.seen = Instant::now();
                held.cwd = PathBuf::from(&session.cwd);
                continue;
            }

            if processes.len() >= 512
                && !processes.contains_key(&key)
                && let Some(oldest) = processes
                    .iter()
                    .min_by_key(|(_, held)| held.seen)
                    .map(|(key, _)| key.clone())
            {
                processes.remove(&oldest);
            }

            let exit = match revision {
                Observation::Process(pid) => match crate::events::exits::Exit::watch(pid) {
                    Ok(exit) => Some(exit),
                    Err(error) => {
                        log::debug!("external process exit cannot be observed: {error}");
                        continue;
                    }
                },
                Observation::Recent(_) => None,
            };
            let hub = self.events.clone();
            let id = session.id.clone();
            let agent = session.agent.parse::<Agent>().ok();
            let pending = source.pending.clone();
            let dirty = source.dirty.clone();
            let occupancy = self.occupancy.clone();
            let task = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
                match exit {
                    // Seen to exit: the session reads as stopped at once, not
                    // as active a moment ago from its last write.
                    Some(exit) => {
                        exit.wait().await;
                        occupancy.note_closed(&id);
                    }
                    None => {
                        if let Some(deadline) = deadline {
                            tokio::time::sleep_until(deadline).await;
                        }
                    }
                }

                // Presence can change without a transcript write. Invalidate the
                // matching listing before another harness's write reuses it.
                if let Some(agent) = agent {
                    crate::lock(&pending).agents.insert(agent);
                    let _ = dirty.try_send(());
                }

                hub.changed(beep_proto::node::events::Topic::keyed("agent", &id));
                hub.changed(beep_proto::node::events::Topic::new("agents"));
            }));
            processes.insert(
                key,
                ProcessWatch {
                    revision,
                    seen: Instant::now(),
                    cwd: PathBuf::from(&session.cwd),
                    _task: task,
                },
            );
        }

        let changed = before != active_roots(&processes);
        drop(processes);

        if changed {
            self.events
                .changed(beep_proto::node::events::Topic::new("agents"));
        }
    }

    pub(in crate::agents) fn observed_work_roots(&self) -> Vec<PathBuf> {
        crate::lock(&self.sources)
            .upgrade()
            .map_or_else(Vec::new, |source| {
                active_roots(&crate::lock(&source.processes))
            })
    }

    pub(crate) fn start_event_bridge(&self) {
        self.event_forward.get_or_init(|| {
            let mut source = crate::events::system().subscribe();
            let hub = self.events.clone();
            tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
                loop {
                    match source.recv().await {
                        Ok(event) => hub.forward(event),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            hub.changed(beep_proto::node::events::Topic::new("*"))
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                    }
                }
            }))
        });
    }

    /// The rings of the agents' logs changing, for the spend scan, which holds
    /// the log observers while it runs.
    pub(crate) async fn log_changes(
        self: &Arc<Self>,
    ) -> anyhow::Result<crate::agents::spend::LogChanges> {
        let rings = self.events.subscribe();
        // Without the watcher nothing rings, and a scan would run once and wait for ever: the
        // stream that asked ends with the error, and its reader opens another.
        let sources = self
            .watch_event_sources()
            .await
            .context("the agents' logs are not watched for spend")?;
        let watching = Some(sources as Arc<dyn std::any::Any + Send + Sync>);

        Ok(crate::agents::spend::LogChanges::new(rings, watching))
    }

    /// Log observers exist only while some client follows agent topics.
    pub(crate) async fn watch_event_sources(self: &Arc<Self>) -> Result<Arc<Sources>> {
        if let Some(held) = crate::lock(&self.sources).upgrade() {
            return Ok(held);
        }

        let roots: Vec<_> = Agent::ALL
            .into_iter()
            .flat_map(|agent| {
                self.follow_roots(agent)
                    .into_iter()
                    .map(move |path| (agent, path))
            })
            .collect();
        let hub = self.events.clone();
        let agents = Arc::downgrade(self);
        let watching = self.occupancy.watching();
        let source = spawn_blocking(move || {
            let roots = roots
                .into_iter()
                .map(|(agent, path)| crate::fs::paths::real(&path).map(|path| (agent, path)))
                .collect::<Result<Vec<_>>>()?;
            let watched: Vec<_> = roots.iter().map(|(_, path)| path.clone()).collect();
            let (dirty, mut summaries) = mpsc::channel(1);
            let pending = Arc::new(Mutex::new(Pending::default()));
            let summary_pending = pending.clone();
            let summary = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
                let mut previous = None;
                let mut cache = SummaryCache::default();
                let mut first = true;

                while summaries.recv().await.is_some() {
                    sleep(Duration::from_millis(400)).await;
                    while summaries.try_recv().is_ok() {}
                    let Some(agents) = agents.upgrade() else {
                        return;
                    };

                    let pending = std::mem::take(&mut *crate::lock(&summary_pending));

                    if !pending.agents.is_empty() {
                        // Unkeyed `agent` stays for log watchers (spend, search, claude).
                        // External session projections listen to harness-scoped `agent-log.<agent>`
                        // so writes in one harness do not dirty unrelated projections.
                        hub.changed(beep_proto::node::events::Topic::new("agent"));
                        for agent in &pending.agents {
                            hub.changed(beep_proto::node::events::Topic::keyed(
                                "agent-log",
                                agent.as_ref(),
                            ));
                        }
                    }

                    // The first read takes every store: what an earlier read
                    // left the occupancy may be older than this watch.
                    let stale = if std::mem::take(&mut first) {
                        Agent::ALL.into_iter().collect()
                    } else {
                        pending.agents
                    };

                    // The read publishes before the ping, so whoever it wakes
                    // reads it.
                    let changed = match cache.read(&agents, &stale).await {
                        Ok(summary) => {
                            if previous.as_ref() == Some(&summary) {
                                false
                            } else {
                                previous = Some(summary);
                                true
                            }
                        }
                        Err(error) => {
                            log::debug!("agent summary could not be reconciled: {error}");
                            // Readers fall back to a direct read and see the error.
                            true
                        }
                    };

                    if changed {
                        hub.changed(beep_proto::node::events::Topic::new("agents"));
                    }
                }
            }));
            let changed = pending.clone();
            let notify = dirty.clone();
            // The first read, so readers have a value to take.
            let _ = dirty.try_send(());
            let paths = crate::events::paths::Paths::with_paths(
                "agent-sessions",
                move |changed_roots, _, changed_paths| {
                    let relevant = {
                        let mut pending = crate::lock(&changed);
                        pending.record_paths(&roots, &changed_roots, changed_paths.as_deref())
                    };

                    if relevant {
                        let _ = notify.try_send(());
                    }
                },
            )?;
            Ok::<_, anyhow::Error>((
                Arc::new(Sources {
                    paths,
                    processes: std::sync::Mutex::default(),
                    pending,
                    dirty,
                    _watching: watching,
                    _summary: summary,
                }),
                watched,
            ))
        })
        .await
        .context("starting session notifications")??;
        let (source, roots) = source;
        source.paths.replace(roots).await?;
        let mut held = crate::lock(&self.sources);

        if let Some(current) = held.upgrade() {
            return Ok(current);
        }
        *held = Arc::downgrade(&source);
        Ok(source)
    }
}

#[cfg(test)]
#[path = "../../tests/agents_follow_sources.rs"]
mod tests;
