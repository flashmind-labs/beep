//! Who started each coding agent session, as the nodes last listed them.
//! Child sessions remain available to the parent panel while top-level
//! lists omit them.

use beep_mesh::machine::listing::Linked;
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
/// session, and its child sessions, by machine.
#[derive(Default)]
pub(super) struct Parents(Mutex<Held>);

#[derive(Default)]
struct Held {
    parents: HashMap<(String, String), SessionParent>,
    children: HashMap<String, Vec<Linked>>,
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

        let children: Vec<Linked> = listed
            .iter()
            .filter(|linked| linked.parent.is_some() || linked.reviewer)
            .cloned()
            .collect();
        held.children.insert(machine.to_owned(), children);
    }

    pub(super) fn of(&self, machine: &str, session: &str) -> Option<SessionParent> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .parents
            .get(&(machine.to_owned(), session.to_owned()))
            .cloned()
    }

    /// Whether `machine` last listed `session` as a reviewer.
    pub(super) fn is_reviewer(&self, machine: &str, session: &str) -> bool {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .children
            .get(machine)
            .is_some_and(|listed| {
                listed
                    .iter()
                    .any(|linked| linked.reviewer && linked.session.id == session)
            })
    }

    /// Child sessions every machine listed last, with their reviewer flag.
    pub(super) fn children(&self) -> Vec<Linked> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .children
            .values()
            .flatten()
            .cloned()
            .collect()
    }
}
