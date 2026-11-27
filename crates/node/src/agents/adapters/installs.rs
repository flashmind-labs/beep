//! How the adapters the node installs itself stand.

use crate::agents::kinds::{AgentKind, Detection};
use crate::status::{Agent, AgentState};
use beep_proto::node::AdapterInstall;
use beep_proto::watched;
use std::collections::BTreeMap;
use tokio::sync::{Notify, watch};

/// How the adapters the node installs stand, by agent name, and Try again.
#[derive(Default)]
pub(crate) struct Installs {
    pub(in crate::agents) states: watch::Sender<BTreeMap<&'static str, Install>>,
    /// Wakes the worker for a detection now rather than in an hour.
    pub(in crate::agents) again: Notify,
}

#[derive(Clone, PartialEq)]
pub(crate) enum Install {
    Running,
    Done,
    Failed(String),
}

impl Installs {
    pub(in crate::agents) fn set(&self, name: &'static str, install: Install) {
        self.states.send_modify(|states| {
            states.insert(name, install);
        });
    }

    /// Takes what a detection found: an agent on the node's own adapter is
    /// done, one still lacking keeps how its last try went, and the others
    /// are forgotten.
    pub(in crate::agents) fn saw(&self, detection: &Detection) {
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
    pub(in crate::agents) fn pending(&self) -> Vec<AdapterInstall> {
        watched::read_sent(&self.states, |states| {
            states
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
        })
    }

    /// `kinds`, then the agents whose adapter is on its way or failed.
    pub(in crate::agents) fn report(&self, kinds: &[AgentKind]) -> Vec<Agent> {
        let installs = watched::read_sent(&self.states, Clone::clone);
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
