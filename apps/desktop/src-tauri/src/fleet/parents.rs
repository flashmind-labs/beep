//! Who started each coding agent session, as the nodes last listed them: a
//! session an agent started with `start_agent` names its parent, which the
//! session lists nest under.

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
/// session.
#[derive(Default)]
pub(super) struct Parents(Mutex<HashMap<(String, String), SessionParent>>);

impl Parents {
    /// Takes `listed`, the sessions of `machine`, in place of what it had.
    pub(super) fn note(&self, machine: &str, listed: &[Linked]) {
        let mut all = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        all.retain(|(held, _), _| held != machine);
        for linked in listed {
            let Some(parent) = &linked.parent else {
                continue;
            };
            let machine_id = parent
                .machine
                .map_or_else(|| machine.to_owned(), |id| id.to_string());
            all.insert(
                (machine.to_owned(), linked.session.id.clone()),
                SessionParent {
                    machine_id,
                    id: parent.session.clone(),
                    key: parent.key.clone(),
                },
            );
        }
    }

    pub(super) fn of(&self, machine: &str, session: &str) -> Option<SessionParent> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&(machine.to_owned(), session.to_owned()))
            .cloned()
    }
}
