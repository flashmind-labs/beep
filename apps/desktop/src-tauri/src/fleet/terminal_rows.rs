//! The terminals the mesh holds, where each one runs and which a window follows, apart from what
//! is done with them: the records under `terminal/<id>`, and the topics a node is asked for.

use super::{Fleet, asked::newer_than, view_signal::Interest};
use crate::error::Result;
use beep_mesh::{Key, machine::terminals::Kept};
use beep_proto::node::{events::Topic, mesh_state::TerminalRecord};
use iroh::EndpointId;
use semver::Version;
use uuid::Uuid;

/// The last released node, which pushes no terminal's screen on a topic of its own: the windows
/// following one are sent it again when that node's terminal or task list changes instead.
pub(crate) const BEFORE_OUTPUT_TOPICS: Version = Version::new(0, 8, 25);

/// One terminal a desktop keeps, as the mesh holds it under `terminal/<id>`.
#[derive(Debug, Clone)]
pub(crate) struct Row {
    pub(crate) id: Uuid,
    pub(crate) record: TerminalRecord,
}

impl Row {
    pub(crate) fn kept(&self) -> Kept<'_> {
        Kept {
            task: self.record.task,
            pty: self.record.pty,
            command: &self.record.command,
        }
    }

    pub(crate) fn key(&self) -> Key {
        Key::Terminal(self.id.to_string())
    }
}

/// The terminals the mesh holds, in no order.
pub(crate) fn rows(fleet: &Fleet) -> Vec<Row> {
    let Some(mesh) = fleet.mesh() else {
        return Vec::new();
    };
    mesh.scan("terminal/")
        .into_iter()
        .filter_map(|(key, value)| {
            let Key::Terminal(id) = key else {
                return None;
            };
            Some(Row {
                id: id.parse().ok()?,
                record: TerminalRecord::from_bytes(&value).ok()?,
            })
        })
        .collect()
}

pub(crate) fn find(fleet: &Fleet, id: Uuid) -> Option<Row> {
    rows(fleet).into_iter().find(|row| row.id == id)
}

/// Writes the terminal to the mesh, where every desktop reads it.
pub(crate) async fn save(fleet: &Fleet, row: &Row) -> Result<()> {
    let bytes = row.record.to_bytes().map_err(anyhow::Error::from)?;
    let mesh = fleet.ready().await?;
    Ok(mesh.set(row.key(), Some(bytes)).await?)
}

pub(crate) async fn forget(fleet: &Fleet, id: Uuid) -> Result<()> {
    let mesh = fleet.ready().await?;
    Ok(mesh.set(Key::Terminal(id.to_string()), None).await?)
}

/// Where a terminal runs: its machine, and its number there among the node's terminals (`pty`)
/// or among its background tasks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Place {
    pub(crate) id: Uuid,
    pub(crate) machine: EndpointId,
    pub(crate) task: u32,
    pub(crate) pty: bool,
}

/// Where each terminal the mesh holds runs.
pub(crate) fn places(fleet: &Fleet) -> Vec<Place> {
    rows(fleet)
        .into_iter()
        .map(|row| Place {
            id: row.id,
            machine: row.record.machine,
            task: row.record.task,
            pty: row.record.pty,
        })
        .collect()
}

/// The terminal a window's topic follows the output of, when it names one.
pub(crate) fn followed(topic: &Topic) -> Option<Uuid> {
    if topic.name() != "terminal-output" || topic.is_pattern() {
        return None;
    }

    topic.key()?.parse().ok()
}

/// Whether `machine` is a node from before the topics of one terminal's screen. This computer's
/// own node is the desktop's build.
pub(crate) fn old_node(fleet: &Fleet, machine: EndpointId) -> bool {
    machine != fleet.me
        && !newer_than(
            fleet.status().version(machine).as_deref(),
            &BEFORE_OUTPUT_TOPICS,
        )
}

/// The topic a node pushes `place`'s screen on, or the list that changes with it on a node from
/// before.
fn node_topic(place: &Place, old: bool) -> Topic {
    match (old, place.pty) {
        (true, true) => Topic::new("terminals"),
        (true, false) => Topic::new("processes"),
        (false, true) => Topic::keyed("terminal-output", place.task.to_string()),
        (false, false) => Topic::keyed("process-output", place.task.to_string()),
    }
}

/// What the nodes are asked for, given what the windows follow: a window follows a terminal by its
/// id, and its node knows it by number, so each such interest becomes the node's topic on the
/// terminal's machine. A terminal the mesh does not hold yet is left out until it does.
pub(crate) fn node_interests(
    interests: Vec<Interest>,
    places: &[Place],
    old: impl Fn(EndpointId) -> bool,
) -> Vec<Interest> {
    interests
        .into_iter()
        .filter_map(|interest| {
            let Some(id) = followed(&interest.topic) else {
                return Some(interest);
            };
            let place = places.iter().find(|place| place.id == id)?;

            Some(Interest {
                machine: Some(place.machine.to_string()),
                topic: node_topic(place, old(place.machine)),
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "../tests/fleet_terminal_rows.rs"]
mod tests;
