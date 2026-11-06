//! Keeps the agents' adapters installed: at start and every hour after, the
//! node detects its agents again, installs the adapters missing, one at a
//! time, and offers what it can start now.

use super::{install, layout};
use crate::{
    agents::{
        AgentKind, Agents,
        kinds::{self, Detection},
    },
    link::local_status::{Agent, AgentState},
};
use beep_proto::node::AdapterInstall;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    sync::{Notify, watch},
    task::spawn_blocking,
};
use tokio_util::task::AbortOnDropHandle;

/// How long the node waits between two detections, and so before it tries
/// a failed install again.
const RECHECK: Duration = Duration::from_secs(60 * 60);

/// How the adapters the node installs stand, by agent name, and Try again.
#[derive(Default)]
pub(crate) struct Installs {
    states: watch::Sender<BTreeMap<&'static str, Install>>,
    /// Wakes the worker for a detection now rather than in an hour.
    again: Notify,
}

#[derive(Clone, PartialEq)]
pub(crate) enum Install {
    Running,
    Done,
    Failed(String),
}

impl Installs {
    fn set(&self, name: &'static str, install: Install) {
        self.states.send_modify(|states| {
            states.insert(name, install);
        });
    }

    /// Takes what a detection found: an agent on the node's own adapter is
    /// done, one still lacking keeps how its last try went, and the others
    /// are forgotten.
    fn saw(&self, detection: &Detection) {
        self.states.send_if_modified(|installs| {
            let before = installs.clone();
            installs.retain(|name, _| detection.lacking.iter().any(|known| known.name == *name));
            for name in &detection.on_own_node {
                installs.insert(name, Install::Done);
            }
            *installs != before
        });
    }

    /// The adapters on their way or failed, for the machine's report.
    fn pending(&self) -> Vec<AdapterInstall> {
        self.states
            .borrow()
            .iter()
            .filter_map(|(name, install)| {
                let failed = match install {
                    Install::Running => None,
                    Install::Failed(reason) => Some(reason.clone()),
                    Install::Done => return None,
                };
                Some(AdapterInstall {
                    agent: (*name).to_owned(),
                    failed,
                })
            })
            .collect()
    }

    /// `kinds`, then the agents whose adapter is on its way or failed.
    fn report(&self, kinds: &[AgentKind]) -> Vec<Agent> {
        let installs = self.states.borrow();
        let mut agents: Vec<Agent> = kinds
            .iter()
            .map(|kind| Agent {
                name: kind.name.clone(),
                state: match installs.get(kind.name.as_str()) {
                    Some(Install::Done) => AgentState::OwnAdapter,
                    _ => AgentState::Ready,
                },
            })
            .collect();
        for (name, install) in installs.iter() {
            if kinds.iter().any(|kind| kind.name == *name) {
                continue;
            }
            let state = match install {
                Install::Running => AgentState::Installing,
                Install::Failed(reason) => AgentState::Failed {
                    reason: reason.clone(),
                },
                Install::Done => continue,
            };
            agents.push(Agent {
                name: (*name).to_owned(),
                state,
            });
        }
        agents
    }
}

impl Agents {
    /// Installs the adapters the agents found here lack into the node's
    /// folder `node_dir`, in the background, now and on every detection
    /// after; `extra` are the agents from `node.toml`. Dropping the handle
    /// stops the work.
    pub fn install_adapters(
        self: &Arc<Self>,
        node_dir: PathBuf,
        extra: Vec<AgentKind>,
    ) -> AbortOnDropHandle<()> {
        let agents = self.clone();
        AbortOnDropHandle::new(tokio::spawn(async move {
            loop {
                agents.keep_adapters(&node_dir, &extra).await;
                tokio::select! {
                    () = tokio::time::sleep(RECHECK) => {}
                    () = agents.adapters.again.notified() => {}
                }
            }
        }))
    }

    /// One detection, and the installs it calls for, one at a time.
    async fn keep_adapters(&self, node_dir: &Path, extra: &[AgentKind]) {
        let Some(detection) = detect(node_dir, extra).await else {
            return;
        };
        self.adapters.saw(&detection);
        self.set_kinds(detection.kinds);
        if detection.lacking.is_empty() {
            return;
        }
        let root = layout::root(node_dir);
        let mut any = false;
        for known in detection.lacking {
            log::info!("{} found, installing its adapter", known.cli);
            self.adapters.set(known.name, Install::Running);
            match install::adapter(&root, known).await {
                Ok(installed) => {
                    log::info!(
                        "{} adapter installed at {}",
                        known.name,
                        installed.script.display()
                    );
                    self.adapters.set(known.name, Install::Done);
                    any = true;
                }
                Err(e) => {
                    log::warn!(
                        "{} adapter not installed, trying again in an hour: {e:#}",
                        known.name
                    );
                    self.adapters
                        .set(known.name, Install::Failed(format!("{e:#}")));
                }
            }
        }
        if !any {
            return;
        }
        let Some(after) = detect(node_dir, extra).await else {
            return;
        };
        self.adapters.saw(&after);
        self.set_kinds(after.kinds);
    }

    /// The agents this node offers and how their adapters stand, for
    /// `beep-node status`.
    pub(crate) fn agent_report(&self) -> Vec<Agent> {
        self.adapters.report(&self.kinds())
    }

    /// The adapters on their way or failed, for `EnvReport::adapters`.
    pub(crate) fn adapter_installs(&self) -> Vec<AdapterInstall> {
        self.adapters.pending()
    }

    /// Changes each time an adapter install starts, ends or is forgotten.
    pub(crate) fn adapters_changed(&self) -> watch::Receiver<BTreeMap<&'static str, Install>> {
        self.adapters.states.subscribe()
    }

    /// Detects and installs the missing adapters now, as Try again asks:
    /// the failed ones read as installing at once.
    pub(crate) fn install_adapters_now(&self) {
        self.adapters.states.send_if_modified(|installs| {
            let mut any = false;
            for install in installs.values_mut() {
                if matches!(install, Install::Failed(_)) {
                    *install = Install::Running;
                    any = true;
                }
            }
            any
        });
        self.adapters.again.notify_one();
    }
}

/// `kinds::detection` off the async workers: it looks on disk.
async fn detect(node_dir: &Path, extra: &[AgentKind]) -> Option<Detection> {
    let node_dir = node_dir.to_owned();
    let extra = extra.to_vec();
    match spawn_blocking(move || kinds::detection(&extra, Some(&node_dir))).await {
        Ok(detection) => Some(detection),
        Err(e) => {
            log::warn!("detecting the coding agents failed: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::kinds::known;

    fn kind(name: &str) -> AgentKind {
        AgentKind {
            name: name.to_owned(),
            program: name.to_owned(),
            args: Vec::new(),
            env: BTreeMap::new(),
            compact: None,
        }
    }

    #[test]
    fn the_report_says_what_waits_on_an_adapter() {
        let installs = Installs::default();
        let claude = known("claude").expect("claude");
        let codex = known("codex").expect("codex");
        installs.set("claude", Install::Failed("needs Node.js: no curl".into()));
        installs.set("codex", Install::Running);
        assert_eq!(
            installs.pending(),
            [
                AdapterInstall {
                    agent: "claude".into(),
                    failed: Some("needs Node.js: no curl".into()),
                },
                AdapterInstall {
                    agent: "codex".into(),
                    failed: None,
                },
            ]
        );
        let report = installs.report(&[kind("gemini")]);
        let states: Vec<(&str, &AgentState)> = report
            .iter()
            .map(|agent| (agent.name.as_str(), &agent.state))
            .collect();
        assert_eq!(
            states,
            [
                ("gemini", &AgentState::Ready),
                (
                    "claude",
                    &AgentState::Failed {
                        reason: "needs Node.js: no curl".into()
                    }
                ),
                ("codex", &AgentState::Installing),
            ]
        );
        // codex's adapter came; claude's still lacks one.
        installs.saw(&Detection {
            kinds: vec![kind("codex")],
            lacking: vec![claude],
            on_own_node: vec![codex.name],
        });
        let report = installs.report(&[kind("codex")]);
        assert_eq!(report[0].state, AgentState::OwnAdapter);
        assert!(matches!(report[1].state, AgentState::Failed { .. }));
    }
}
