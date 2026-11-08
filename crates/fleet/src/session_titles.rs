//! User titles kept apart from the titles refreshed by coding agents.

use crate::{
    error::{FleetError, Result},
    records::{read, write},
};
use beep_mesh::{Key, Mesh};
use beep_proto::{MachineSession, node::mesh_state::StateValue};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Title(String);
impl StateValue for Title {}

fn key(machine: &str, agent: &str, session: &str) -> Key {
    Key::Other(format!("agent-title/{machine}/{agent}/{session}"))
}

/// The user's title for this session, independent of its latest generated title.
pub fn get(mesh: &Mesh, machine: &str, agent: &str, session: &str) -> Option<String> {
    read::<Title>(mesh, &key(machine, agent, session)).map(|title| title.0)
}

/// Saves an override; an empty title restores the agent's title.
pub async fn save(
    mesh: &Mesh,
    machine: &str,
    agent: &str,
    session: &str,
    title: &str,
) -> Result<()> {
    let title = title.trim();
    if title.chars().count() > 200 || title.chars().any(char::is_control) {
        return Err(FleetError::refused(
            "Use a single line of at most 200 characters.",
        ));
    }
    if machine.is_empty()
        || machine.contains('/')
        || agent.is_empty()
        || agent.contains('/')
        || session.is_empty()
    {
        return Err(FleetError::refused("The session identity is invalid."));
    }
    let value = (!title.is_empty()).then(|| Title(title.to_owned()));
    write(mesh, key(machine, agent, session), value.as_ref()).await
}

/// Applies the saved title after a session is refreshed from its machine.
pub fn apply(mesh: &Mesh, session: &mut MachineSession) {
    if let Some(title) = get(mesh, &session.machine_id, &session.agent, &session.id) {
        session.title = Some(title);
    }
}
