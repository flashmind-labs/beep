//! Event sources for sessions on this machine.

use super::{recent_deadline, source_summary};
use crate::agents::{Agents, found::Agent};
use anyhow::{Context, Result};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    sync::mpsc,
    task::spawn_blocking,
    time::{Instant, sleep},
};

pub(crate) struct Sources {
    paths: crate::events::paths::Paths,
    processes: std::sync::Mutex<std::collections::HashMap<(String, String), ProcessWatch>>,
    _summary: tokio_util::task::AbortOnDropHandle<()>,
}

#[derive(PartialEq, Eq)]
enum Observation {
    Process(u32),
    Recent(u64),
}

struct ProcessWatch {
    revision: Observation,
    seen: Instant,
    cwd: PathBuf,
    _task: tokio_util::task::AbortOnDropHandle<()>,
}

fn active_roots(
    processes: &std::collections::HashMap<(String, String), ProcessWatch>,
) -> Vec<PathBuf> {
    let mut roots: Vec<_> = processes
        .values()
        .filter(|held| {
            matches!(held.revision, Observation::Process(_)) && !held._task.is_finished()
        })
        .map(|held| held.cwd.clone())
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

impl Agents {
    pub(in crate::agents) fn observe_external(
        &self,
        sessions: &[beep_proto::node::agent::ExternalSession],
    ) {
        let Some(source) = crate::lock(&self.event_watcher).upgrade() else {
            return;
        };
        let mut processes = crate::lock(&source.processes);
        let before = active_roots(&processes);
        processes.retain(|_, held| !held._task.is_finished());

        for session in sessions {
            let key = (session.agent.clone(), session.id.clone());
            let revision = match (session.pid, recent_deadline(session)) {
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
            let deadline = recent_deadline(session);
            let hub = self.events.clone();
            let id = session.id.clone();
            let task = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
                match exit {
                    Some(exit) => exit.wait().await,
                    None => {
                        if let Some(deadline) = deadline {
                            tokio::time::sleep_until(deadline).await;
                        }
                    }
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
        crate::lock(&self.event_watcher)
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

    /// Log observers exist only while some client follows agent topics.
    pub(crate) async fn watch_event_sources(self: &Arc<Self>) -> Result<Arc<Sources>> {
        if let Some(held) = crate::lock(&self.event_watcher).upgrade() {
            return Ok(held);
        }

        let roots: Vec<_> = [Agent::Codex, Agent::Pi, Agent::Flsh, Agent::OpenCode]
            .into_iter()
            .flat_map(|agent| self.stores.follow_paths(Some(agent)))
            .chain(self.external.follow_paths())
            .collect();
        let hub = self.events.clone();
        let agents = Arc::downgrade(self);
        let source = spawn_blocking(move || {
            let (dirty, mut summaries) = mpsc::channel(1);
            let structure = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let summary_structure = structure.clone();
            let summary_hub = hub.clone();
            let summary = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
                let mut previous = None;

                while summaries.recv().await.is_some() {
                    sleep(Duration::from_millis(400)).await;
                    while summaries.try_recv().is_ok() {}
                    let Some(agents) = agents.upgrade() else {
                        return;
                    };

                    if summary_structure.swap(false, std::sync::atomic::Ordering::AcqRel) {
                        *agents.sighted.lock().await = None;
                    }

                    match source_summary(&agents).await {
                        Ok(summary) if previous.as_ref() != Some(&summary) => {
                            previous = Some(summary);
                            summary_hub.changed(beep_proto::node::events::Topic::new("agents"));
                        }
                        Ok(_) => {}
                        Err(error) => {
                            log::debug!("agent summary could not be reconciled: {error}");
                            summary_hub.changed(beep_proto::node::events::Topic::new("agents"));
                        }
                    }
                }
            }));
            let paths = crate::events::paths::Paths::with_structure(move |_, structural| {
                if structural {
                    structure.store(true, std::sync::atomic::Ordering::Release);
                }

                hub.changed(beep_proto::node::events::Topic::new("agent"));
                let _ = dirty.try_send(());
            })?;
            Ok::<_, anyhow::Error>(Arc::new(Sources {
                paths,
                processes: std::sync::Mutex::default(),
                _summary: summary,
            }))
        })
        .await
        .context("starting session notifications")??;
        source.paths.replace(roots).await?;
        let mut held = crate::lock(&self.event_watcher);

        if let Some(current) = held.upgrade() {
            return Ok(current);
        }
        *held = Arc::downgrade(&source);
        Ok(source)
    }
}
