//! Teams in the mesh's state: the `beepteam1` ticket that invites a person in
//! another mesh, and the writes every desktop makes alike. The inviter's
//! desktop keeps the invite's expiry and enforces it when the secret is spent.

use crate::{
    JoinSecret, Key, Mesh, MeshError, TicketError,
    values::{read, scan},
};
use beep_proto::{
    lenient,
    node::mesh_state::StateValue,
    team::{ContactRecord, TeamRecord, TeamRepo},
};
use iroh::EndpointId;
use iroh_tickets::{ParseError, Ticket};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tokio::sync::{Mutex as AsyncMutex, MutexGuard};

/// Every read-modify-write of `team/` and `contact/` in this process holds it.
static TEAM_WRITES: AsyncMutex<()> = AsyncMutex::const_new(());

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TeamTicket {
    #[serde(with = "crate::wire::id")]
    pub inviter: EndpointId,
    pub repo: TeamRepo,
    pub secret: JoinSecret,
}

impl TeamTicket {
    /// Reads the code in either case: a QR code carries it in capitals.
    pub fn parse(text: &str) -> Result<Self, TicketError> {
        Self::decode_string(&text.trim().to_ascii_lowercase()).map_err(|_| TicketError::Unreadable)
    }
}

impl Ticket for TeamTicket {
    const KIND: &'static str = "beepteam1";

    fn encode_bytes(&self) -> Vec<u8> {
        rmp_serde::to_vec(self).expect("msgpack encodes a team ticket")
    }

    fn decode_bytes(bytes: &[u8]) -> Result<Self, ParseError> {
        lenient::from_slice(bytes)
            .map_err(|_| ParseError::verification_failed("the ticket cannot be read"))
    }
}

/// What the inviter stores in place of a spent invite's secret.
pub fn secret_hash(secret: &JoinSecret) -> [u8; 32] {
    *blake3::hash(secret.as_bytes()).as_bytes()
}

/// The team writes held: what changing `team/` and `contact/` takes, so a
/// desktop's change and its node's never read and write over each other.
pub struct TeamWrites {
    _guard: MutexGuard<'static, ()>,
}

impl TeamWrites {
    /// Waits for the writes in progress, then holds them until dropped.
    pub async fn lock() -> Self {
        Self {
            _guard: TEAM_WRITES.lock().await,
        }
    }
}

/// Writes `pending`, a team record a node changed, over the mesh's own: its
/// members and removals count, the invites and settings stay. A removal on
/// either side wins, so a stale change never brings anyone back. A team the
/// mesh holds no record of is not written: it was left. One a teammate
/// removed this mesh from is left too: its record and the contacts only it
/// brought are deleted. Whether it wrote.
pub async fn take_roster(
    mesh: &Mesh,
    pending: TeamRecord,
    held: &TeamWrites,
) -> Result<bool, MeshError> {
    let key = Key::Team(pending.repo.clone());
    let Some(current) = read::<TeamRecord>(mesh, &key) else {
        return Ok(false);
    };
    let mut next = current.clone();

    for gone in pending.removed {
        if !next.removed.contains(&gone) {
            next.removed.push(gone);
        }
    }

    next.members = pending
        .members
        .into_iter()
        .filter(|member| {
            let mut removed = next.removed.iter();
            !removed.any(|gone| gone.login == member.login && gone.mesh == member.mesh)
        })
        .collect();

    if !next.members.iter().any(|member| member.mesh == mesh.id()) {
        mesh.set(key, None).await?;
        keep_contacts(mesh, held).await?;
        return Ok(true);
    }
    if next == current {
        return Ok(false);
    }

    mesh.set(key, Some(next.to_bytes()?)).await?;
    Ok(true)
}

/// Keeps a `contact/<mesh>` record for each member of another mesh on this
/// mesh's teams: the login they hold on each team and all their devices. A
/// contact who shares no team any more is deleted.
pub async fn keep_contacts(mesh: &Mesh, _held: &TeamWrites) -> Result<(), MeshError> {
    let own = mesh.id();
    let mut contacts: BTreeMap<EndpointId, ContactRecord> = BTreeMap::new();

    for (_, record) in scan::<TeamRecord>(mesh, "team/") {
        for member in record
            .members
            .into_iter()
            .filter(|member| member.mesh != own)
        {
            let contact = contacts
                .entry(member.mesh)
                .or_insert_with(|| ContactRecord {
                    mesh: member.mesh,
                    logins: Vec::new(),
                    devices: Vec::new(),
                });
            contact.logins.push((record.repo.clone(), member.login));

            for device in member.devices {
                if !contact.devices.iter().any(|have| have.id == device.id) {
                    contact.devices.push(device);
                }
            }
        }
    }

    let held: Vec<EndpointId> = mesh
        .scan("contact/")
        .into_iter()
        .filter_map(|(key, _)| match key {
            Key::Contact(id) => Some(id),
            _ => None,
        })
        .collect();

    for id in held.into_iter().filter(|id| !contacts.contains_key(id)) {
        mesh.set(Key::Contact(id), None).await?;
    }

    for (id, contact) in contacts {
        let key = Key::Contact(id);

        if read::<ContactRecord>(mesh, &key).as_ref() != Some(&contact) {
            mesh.set(key, Some(contact.to_bytes()?)).await?;
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "tests/team.rs"]
mod tests;
