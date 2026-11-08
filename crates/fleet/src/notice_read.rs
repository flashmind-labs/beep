//! Which notifications the user has read, shared by every app of the mesh. Each app writes the
//! marks of its own reading under `notice-read/<its id>` and takes the newest mark of each
//! notification from all of them, so reading on one app reads it on the rest. Values are
//! positional msgpack: a field is added only at the end with `#[serde(default)]`.

use crate::{
    error::Result,
    records::{WRITES, read, write},
};
use beep_mesh::{Key, Mesh};
use beep_proto::node::mesh_state::StateValue;
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// The most marks one app keeps.
const KEEP: usize = 200;

/// A notification read at `at_ms`. A notification made after that is a new one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Mark {
    id: String,
    at_ms: u64,
}

/// `notice-read/<app>`.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Marks {
    marks: Vec<Mark>,
}

impl StateValue for Marks {}

/// The newest time each notification was read, on any app that is a member.
pub fn read_marks(mesh: &Mesh) -> HashMap<String, u64> {
    let mut newest: HashMap<String, u64> = HashMap::new();
    for (key, bytes) in mesh.scan("notice-read/") {
        let Key::NoticeRead(app) = &key else { continue };
        if !app
            .parse::<EndpointId>()
            .is_ok_and(|app| mesh.is_member(app))
        {
            continue;
        }
        let Ok(held) = Marks::from_bytes(&bytes) else {
            log::warn!("ignored {key} in the mesh: it does not read");
            continue;
        };
        for mark in held.marks {
            let at = newest.entry(mark.id).or_default();
            *at = (*at).max(mark.at_ms);
        }
    }
    newest
}

/// Marks `ids` read at `now_ms` in this app's key. Marks of notifications not in `listed` are
/// dropped, and at most `KEEP` of the newest stay.
pub async fn mark_read(
    mesh: &Mesh,
    ids: &[String],
    listed: &HashSet<String>,
    now_ms: u64,
) -> Result<()> {
    let _one = WRITES.lock().await;
    let key = Key::NoticeRead(mesh.me().to_string());
    let held = read::<Marks>(mesh, &key).unwrap_or_default();
    let marks = merge(held.marks, ids, listed, now_ms);
    write(mesh, key, Some(&Marks { marks })).await
}

fn merge(held: Vec<Mark>, ids: &[String], listed: &HashSet<String>, now_ms: u64) -> Vec<Mark> {
    let mut marks: Vec<Mark> = held
        .into_iter()
        .filter(|mark| !ids.contains(&mark.id))
        .chain(ids.iter().map(|id| Mark {
            id: id.clone(),
            at_ms: now_ms,
        }))
        .filter(|mark| listed.contains(&mark.id))
        .collect();
    marks.sort_by_key(|mark| std::cmp::Reverse(mark.at_ms));
    marks.truncate(KEEP);
    marks
}

#[cfg(test)]
#[path = "tests/notice_read.rs"]
mod tests;
