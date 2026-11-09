//! Who started each coding agent session, as the nodes last listed them: a
//! session an agent started with `start_agent` names its parent, which the
//! session lists nest under, and the reviewers running as a child.

use beep_mesh::machine::listing::Linked;
use beep_proto::MachineSession;
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

/// The session that started another: where it runs and the ids it may be
/// listed under.
#[derive(Debug, Clone, Serialize)]
pub struct SessionParent {
    pub machine_id: String,
    /// The agent's own id for the parent.
    pub id: String,
    /// The id its node listed it under before the agent's own replaced it.
    pub key: String,
}

/// The parents of the sessions each machine listed last, by machine and
/// session, and the reviewers it listed, by machine.
#[derive(Default)]
pub(super) struct Parents(Mutex<Held>);

#[derive(Default)]
struct Held {
    parents: HashMap<(String, String), SessionParent>,
    reviewers: HashMap<String, Vec<MachineSession>>,
}

impl Parents {
    /// Takes `listed`, the sessions of `machine`, in place of what it had.
    pub(super) fn note(&self, machine: &str, listed: &[Linked]) {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        held.parents.retain(|(at, _), _| at != machine);

        for linked in listed {
            let Some(parent) = &linked.parent else {
                continue;
            };
            let machine_id = parent
                .machine
                .map_or_else(|| machine.to_owned(), |id| id.to_string());
            held.parents.insert(
                (machine.to_owned(), linked.session.id.clone()),
                SessionParent {
                    machine_id,
                    id: parent.session.clone(),
                    key: parent.key.clone(),
                },
            );
        }

        let reviewers: Vec<MachineSession> = listed
            .iter()
            .filter(|linked| linked.reviewer)
            .map(|linked| linked.session.clone())
            .collect();
        held.reviewers.insert(machine.to_owned(), reviewers);
    }

    pub(super) fn of(&self, machine: &str, session: &str) -> Option<SessionParent> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .parents
            .get(&(machine.to_owned(), session.to_owned()))
            .cloned()
    }

    /// The reviewers every machine listed last.
    pub(super) fn reviewers(&self) -> Vec<MachineSession> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .reviewers
            .values()
            .flatten()
            .cloned()
            .collect()
    }
}
