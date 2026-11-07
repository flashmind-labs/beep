//! Native session reports replace the matching rows of cached scans.

use super::Fleet;
use beep_mesh::machine::listing::{Host, apply_presence};
use beep_proto::{MachineSession, node::agent::ExternalSession};
use iroh::EndpointId;
use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

#[derive(Default)]
pub(super) struct Presence(Mutex<HashMap<EndpointId, Vec<ExternalSession>>>);

impl Fleet {
    pub(crate) fn note_presence(&self, machine: EndpointId, sessions: Vec<ExternalSession>) {
        self.presence
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(machine, sessions);
    }

    pub(crate) fn apply_presence(&self, sessions: &mut Vec<MachineSession>) {
        let reported = self
            .presence
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        for (machine, presence) in reported {
            if self.member(&machine.to_string()).is_some() {
                apply_presence(
                    &Host {
                        id: machine.to_string(),
                        name: self.name_of(machine),
                    },
                    &presence,
                    sessions,
                );
            }
        }
    }
}
