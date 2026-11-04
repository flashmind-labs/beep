//! What the desktop keeps in the mesh's replicated state: projects, groups, pins, and the
//! saved text of the user's instructions. Sync, credentials and agent settings use the types the
//! nodes read (`beep_proto::node::mesh_state`). Values are msgpack maps with named fields, so a
//! field may be added with `#[serde(default)]`.

use crate::AsyncMutex;
use crate::error::Result;
use beep_mesh::{Key, Mesh};
use beep_proto::node::mesh_state::StateValue;
use chrono::{DateTime, Utc};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

/// Every read-modify-write of a record takes this, so two calls in this process never overwrite
/// each other's change.
pub(crate) static WRITES: AsyncMutex<()> = AsyncMutex::const_new(());

/// `project/<org/repo>`: that a project exists, and its repository's web page. Where it is
/// checked out is one key per machine, `project/<org/repo>/<machine>`, so two desktops that
/// import it on different machines never overwrite each other.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRecord {
    /// The repository's web page.
    pub url: String,
}

/// One machine's checkout of a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Place {
    pub folder: String,
    pub imported_ms: u64,
    /// The user's fork cloned as `origin`, as `org/repo`.
    #[serde(default)]
    pub fork: Option<String>,
}

/// `group/<id>`: folders the sidebar lists projects under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupRecord {
    pub name: String,
    pub paths: Vec<String>,
    pub created_ms: u64,
}

/// `pin/<project>`: a project pinned to the top of the sidebar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinRecord {
    pub at_ms: u64,
}

/// One saved version of the user's instructions. Each is its own key, `instructions/<ms>.<id>`, so
/// two desktops saving at once keep both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedVersion {
    pub text: String,
    pub note: String,
    pub saved_ms: u64,
}

impl StateValue for ProjectRecord {}
impl StateValue for GroupRecord {}
impl StateValue for PinRecord {}
impl StateValue for Place {}
impl StateValue for SavedVersion {}

/// A project as the mesh holds it.
pub(crate) struct Stored {
    pub(crate) name: String,
    pub(crate) record: ProjectRecord,
    pub(crate) places: BTreeMap<EndpointId, Place>,
}

/// How many `/` a key's name holds: one for a project, two for a place of it.
fn slashes(name: &str) -> usize {
    name.matches('/').count()
}

/// Every project, with its places.
pub(crate) fn projects(mesh: &Mesh) -> Vec<Stored> {
    let mut found: BTreeMap<String, Stored> = BTreeMap::new();
    let mut places = Vec::new();
    for (key, bytes) in mesh.scan("project/") {
        let Key::Project(name) = &key else { continue };
        match slashes(name) {
            1 => {
                if let Some(record) = decode::<ProjectRecord>(&key, &bytes) {
                    found.insert(
                        name.clone(),
                        Stored {
                            name: name.clone(),
                            record,
                            places: BTreeMap::new(),
                        },
                    );
                }
            }
            2 => {
                let Some((project, machine)) = name.rsplit_once('/') else {
                    continue;
                };
                let (Ok(machine), Some(place)) =
                    (machine.parse::<EndpointId>(), decode::<Place>(&key, &bytes))
                else {
                    continue;
                };
                places.push((project.to_owned(), machine, place));
            }
            _ => {}
        }
    }
    for (project, machine, place) in places {
        if let Some(stored) = found.get_mut(&project) {
            stored.places.insert(machine, place);
        }
    }
    found.into_values().collect()
}

/// The project called `name`, in any case.
pub(crate) fn find_project(mesh: &Mesh, name: &str) -> Option<Stored> {
    projects(mesh)
        .into_iter()
        .find(|stored| stored.name.eq_ignore_ascii_case(name.trim()))
}

pub(crate) fn project_key(name: &str) -> Key {
    Key::Project(name.to_owned())
}

pub(crate) fn place_key(name: &str, machine: EndpointId) -> Key {
    Key::Project(format!("{name}/{machine}"))
}

/// The value at `key`, `None` when there is none or it does not read.
pub(crate) fn read<T: StateValue>(mesh: &Mesh, key: &Key) -> Option<T> {
    let bytes = mesh.get(key)?;
    decode(key, &bytes)
}

/// Every value under `prefix` that reads; one that does not is left out and its key logged.
pub(crate) fn scan<T: StateValue>(mesh: &Mesh, prefix: &str) -> Vec<(Key, T)> {
    mesh.scan(prefix)
        .into_iter()
        .filter_map(|(key, bytes)| {
            let value = decode(&key, &bytes)?;
            Some((key, value))
        })
        .collect()
}

fn decode<T: StateValue>(key: &Key, bytes: &[u8]) -> Option<T> {
    match T::from_bytes(bytes) {
        Ok(value) => Some(value),
        Err(_) => {
            log::warn!("ignored {key} in the mesh: it does not read");
            None
        }
    }
}

/// Writes `value` at `key`, or deletes the key with `None`.
pub(crate) async fn write<T: StateValue>(mesh: &Mesh, key: Key, value: Option<&T>) -> Result<()> {
    let bytes = value
        .map(StateValue::to_bytes)
        .transpose()
        .map_err(anyhow::Error::from)?;
    mesh.set(key, bytes).await?;
    Ok(())
}

pub(crate) fn now_ms() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX)
}

pub(crate) fn at(ms: u64) -> DateTime<Utc> {
    i64::try_from(ms)
        .ok()
        .and_then(DateTime::from_timestamp_millis)
        .unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
}

pub(crate) fn time(ms: u64) -> SystemTime {
    UNIX_EPOCH + std::time::Duration::from_millis(ms)
}
