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

const PREFIX: &str = "agent-title/";

fn key(machine: &str, agent: &str, session: &str) -> Key {
    Key::Other(format!("{PREFIX}{machine}/{agent}/{session}"))
}

/// A session whose user title changed, and its title now: none when the
/// agent's own title shows again.
#[derive(Debug, Clone, Serialize)]
pub struct Retitled {
    pub machine_id: String,
    pub agent: String,
    pub session: String,
    pub title: Option<String>,
}

/// The title change a changed mesh `key` announces, if it is a title's key.
pub fn changed(mesh: &Mesh, key: &Key) -> Option<Retitled> {
    let Key::Other(text) = key else { return None };
    let mut parts = text.strip_prefix(PREFIX)?.splitn(3, '/');
    let (machine_id, agent, session) = (parts.next()?, parts.next()?, parts.next()?);
    let title = get(mesh, machine_id, agent, session);

    Some(Retitled {
        machine_id: machine_id.to_owned(),
        agent: agent.to_owned(),
        session: session.to_owned(),
        title,
    })
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
