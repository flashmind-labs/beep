//! Which notifications the user has read, shared by every app of the mesh. Each app writes the
//! marks of its own reading under `notice-read/<its id>` and reads the marks of all of them, so
//! reading on one app reads it on the rest. A mark names the notification and a digest of what it
//! said, so a later turn under the same id is unread. Values are positional msgpack: a field is
//! added only at the end with `#[serde(default)]`.

use crate::{
    error::Result,
    records::{WRITES, read, write},
};
use beep_mesh::{Key, Mesh};
use beep_proto::node::mesh_state::StateValue;
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// The most marks one app keeps.
const KEEP: usize = 200;

/// A stable 64-bit digest of the `parts` that make one version of a notification.
pub fn digest(parts: &[&str]) -> u64 {
    let mut hash = blake3::Hasher::new();
    for part in parts {
        hash.update(&(part.len() as u64).to_le_bytes());
        hash.update(part.as_bytes());
    }
    let first = hash.finalize();
    let mut eight = [0; 8];
    eight.copy_from_slice(&first.as_bytes()[..8]);
    u64::from_le_bytes(eight)
}

/// The notification `id` whose content hashes to `digest`, read at `at_ms`. The time only orders
/// marks for the cap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Mark {
    id: String,
    at_ms: u64,
    digest: u64,
}

/// A notification read: its id and the digest of what it said.
pub type Read = (String, u64);

/// `notice-read/<app>`.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Marks {
    marks: Vec<Mark>,
}

impl StateValue for Marks {}

/// Every notification read on any app that is a member.
pub fn read_marks(mesh: &Mesh) -> HashSet<Read> {
    let mut reads = HashSet::new();
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
        reads.extend(held.marks.into_iter().map(|mark| (mark.id, mark.digest)));
    }
    reads
}

/// Marks `reads` read at `now_ms` in this app's key. Marks of notifications not in `listed` are
/// dropped, and at most `KEEP` of the newest stay.
pub async fn mark_read(
    mesh: &Mesh,
    reads: &[Read],
    listed: &HashSet<String>,
    now_ms: u64,
) -> Result<()> {
    let _one = WRITES.lock().await;
    let key = Key::NoticeRead(mesh.me().to_string());
    let held = read::<Marks>(mesh, &key).unwrap_or_default();
    let marks = merge(held.marks, reads, listed, now_ms);
    write(mesh, key, Some(&Marks { marks })).await
}

fn merge(held: Vec<Mark>, reads: &[Read], listed: &HashSet<String>, now_ms: u64) -> Vec<Mark> {
    let mut marks: Vec<Mark> = held
        .into_iter()
        .filter(|mark| !reads.iter().any(|(id, _)| *id == mark.id))
        .chain(reads.iter().map(|(id, digest)| Mark {
            id: id.clone(),
            at_ms: now_ms,
            digest: *digest,
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
