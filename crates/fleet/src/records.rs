//! What the desktop keeps in the mesh's replicated state: projects, groups, pins, and the
//! saved text of the user's instructions. Sync, credentials and agent settings use the types the
//! nodes read (`beep_proto::node::mesh_state`). Values are compact msgpack, so a field is added
//! only at the end with `#[serde(default)]`.

use crate::AsyncMutex;
use crate::error::Result;
pub(crate) use beep_mesh::values::{decode, read, scan};
use beep_mesh::{Key, Mesh};
pub(crate) use beep_proto::node::mesh_state::Place;
use beep_proto::{
    Credential,
    node::mesh_state::{ProjectSyncSettings, StateValue, SyncSpec},
};
use chrono::{DateTime, Utc};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Every read-modify-write of a record takes this, so two calls in this process never overwrite
/// each other's change.
pub(crate) static WRITES: AsyncMutex<()> = AsyncMutex::const_new(());

/// `project/<org/repo>`. Where it is checked out is one key per machine,
/// `project/<org/repo>/<machine>`, so two desktops that import it on different machines never
/// overwrite each other.
pub use beep_proto::node::mesh_state::ProjectRecord;

/// What an `unmoved/...` record says: a value an account moved from a server held that the mesh
/// has none of. Settings lists it to enter again, and a node counts it as the mesh holding that
/// family.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) enum Unmoved {
    /// A credential, shown with its last four characters.
    Credential(Credential),
    /// The name of an env var of a harness, never its value.
    HarnessEnv { agent: String, name: String },
}

impl StateValue for Unmoved {}

pub(crate) const UNMOVED_CREDENTIAL: &str = "unmoved/credential/";

pub(crate) fn unmoved_credential(id: &str) -> Key {
    Key::Unmoved(format!("credential/{id}"))
}

pub(crate) fn unmoved_env(agent: &str, name: &str) -> Key {
    Key::Unmoved(format!("harness/{agent}/{name}"))
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

pub use beep_proto::node::mesh_state::SavedVersion;

impl StateValue for GroupRecord {}
impl StateValue for PinRecord {}

/// A project as the mesh holds it.
pub(crate) struct Stored {
    pub(crate) name: String,
    pub(crate) record: ProjectRecord,
    pub(crate) places: BTreeMap<EndpointId, Place>,
}

/// Every project, with its places.
pub(crate) fn projects(mesh: &Mesh) -> Vec<Stored> {
    let mut found: BTreeMap<String, Stored> = BTreeMap::new();
    let mut places = Vec::new();

    for (key, bytes) in mesh.scan("project/") {
        let Key::Project(name) = &key else { continue };

        // One `/` in a key's name is a project, two a place of it.
        match name.matches('/').count() {
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

/// Keeps `stored` in step on every machine of the mesh with a checkout: its `sync/` record names
/// them once there are two, and what else the record says stays. A record with no machines is a
/// project pinned to where it is, which never syncs. The caller holds `WRITES`.
pub(crate) async fn sync_checkouts(mesh: &Mesh, stored: &Stored) -> Result<()> {
    let places: Vec<(&EndpointId, &Place)> = stored
        .places
        .iter()
        .filter(|(machine, _)| mesh.is_member(**machine))
        .collect();
    if places.len() < 2 {
        return Ok(());
    }

    let key = Key::Sync(stored.name.clone());
    let mut spec = SyncSpec::default();

    if mesh.get(&key).is_some() {
        // One that does not read may say more than this build knows: it stays.
        let Some(held) = read::<SyncSpec>(mesh, &key) else {
            return Ok(());
        };

        if held.folders.is_empty() {
            return Ok(());
        }
        spec = held;
    }

    let before = spec.clone();

    for (machine, place) in places {
        spec.folders.insert(*machine, place.folder.clone());
    }

    if !read::<ProjectSyncSettings>(mesh, &Key::ProjectSync)
        .unwrap_or_default()
        .enabled
    {
        spec.paused = true;
    }

    if spec != before {
        write(mesh, key, Some(&spec)).await?;
    }

    Ok(())
}

pub(crate) fn project_key(name: &str) -> Key {
    Key::Project(name.to_owned())
}

pub(crate) fn place_key(name: &str, machine: EndpointId) -> Key {
    Key::Project(format!("{name}/{machine}"))
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
    UNIX_EPOCH + Duration::from_millis(ms)
}
