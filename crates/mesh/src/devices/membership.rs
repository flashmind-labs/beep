//! Membership and authorization over signed operations.

use super::{Device, DeviceKind, DeviceOp, MeshError, OpBody};
use crate::state::now_ms;
use iroh::{EndpointId, SecretKey};
use std::collections::{BTreeMap, BTreeSet};

/// Ops held while waiting for their author to count.
const MAX_PENDING: usize = 256;

/// How far a device's replicated writes are accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteLimit {
    /// Not a desktop of this mesh.
    Refused,
    /// A removed desktop: entries stamped at or before this time still count,
    /// so a fresh peer converges on what it wrote while a member.
    Until(u64),
    /// A current desktop.
    Always,
}

/// What a merge did. Refused ops never hide the ones that counted.
#[derive(Debug, Default)]
pub struct Merged {
    pub added: Vec<EndpointId>,
    pub removed: Vec<EndpointId>,
    /// Ops of this call kept until their author counts.
    pub held: usize,
    /// Ops of this call that were refused, and why.
    pub rejected: Vec<MeshError>,
}

/// A device and the op that put it in the list. When two desktops add the
/// same key, the earlier `(added_at_ms, author)` wins, whatever the arrival order.
#[derive(Clone, Debug)]
struct Entry {
    device: Device,
    author: EndpointId,
}

impl Entry {
    /// Smaller wins: a desktop beats a node, then the earlier add.
    fn rank(&self) -> (bool, u64, EndpointId) {
        (
            self.device.kind != DeviceKind::Desktop,
            self.device.added_at_ms,
            self.author,
        )
    }
}

#[derive(Clone, Debug)]
pub struct Devices {
    mesh: EndpointId,
    ops: Vec<DeviceOp>,
    pending: Vec<DeviceOp>,
    added: BTreeMap<EndpointId, Entry>,
    /// Removed key and the earliest `at_ms` of a `Remove` naming it.
    removed: BTreeMap<EndpointId, u64>,
}

impl Devices {
    fn empty(mesh: EndpointId) -> Self {
        Self {
            mesh,
            ops: Vec::new(),
            pending: Vec::new(),
            added: BTreeMap::new(),
            removed: BTreeMap::new(),
        }
    }

    /// Starts a mesh. A `Node` genesis is built but refused by `from_ops`
    /// and by every other member's `merge`.
    pub fn genesis(key: &SecretKey, name: &str, kind: DeviceKind) -> Devices {
        let device = Device {
            id: key.public(),
            name: name.to_owned(),
            kind,
            added_at_ms: now_ms(),
        };
        let mut list = Self::empty(key.public());
        let op = DeviceOp::sign(key, key.public(), OpBody::Genesis(device))
            .expect("msgpack encodes a device op");
        list.insert(op);
        list
    }

    /// Builds a list from ops received from a peer. It fails only without a
    /// valid first genesis from a desktop; other ops that were refused or are
    /// held come back in the `Merged`. Compare `mesh()` with the ticket's.
    pub fn from_ops(ops: &[DeviceOp]) -> Result<(Devices, Merged), MeshError> {
        let genesis = ops
            .iter()
            .find(|op| matches!(op.body, OpBody::Genesis(_)))
            .ok_or(MeshError::NoGenesis)?;
        genesis.verify(genesis.author)?;

        if let OpBody::Genesis(device) = &genesis.body
            && (device.id != genesis.author || device.kind != DeviceKind::Desktop)
        {
            return Err(MeshError::NotDesktop(genesis.author));
        }

        let mut list = Self::empty(genesis.author);
        let merged = list.merge(ops);
        Ok((list, merged))
    }

    pub fn mesh(&self) -> EndpointId {
        self.mesh
    }

    pub fn ops(&self) -> &[DeviceOp] {
        &self.ops
    }

    pub fn is_member(&self, id: EndpointId) -> bool {
        self.added.contains_key(&id) && !self.removed.contains_key(&id)
    }

    /// A key that was removed and can never rejoin.
    pub fn is_removed(&self, id: EndpointId) -> bool {
        self.removed.contains_key(&id)
    }

    pub fn is_desktop(&self, id: EndpointId) -> bool {
        self.is_member(id)
            && self
                .added
                .get(&id)
                .is_some_and(|e| e.device.kind == DeviceKind::Desktop)
    }

    /// Which entries from `id` may still be taken.
    pub fn write_limit(&self, id: &EndpointId) -> WriteLimit {
        let Some(entry) = self.added.get(id) else {
            return WriteLimit::Refused;
        };

        if entry.device.kind != DeviceKind::Desktop {
            return WriteLimit::Refused;
        }

        match self.removed.get(id) {
            None => WriteLimit::Always,
            Some(at_ms) => WriteLimit::Until(*at_ms),
        }
    }

    pub fn members(&self) -> Vec<Device> {
        self.added
            .iter()
            .filter(|(id, _)| !self.removed.contains_key(*id))
            .map(|(_, entry)| entry.device.clone())
            .collect()
    }

    /// Signs an `Add` as `key`, which must be a member desktop, and applies it.
    pub fn add(&mut self, key: &SecretKey, device: Device) -> Result<DeviceOp, MeshError> {
        self.gate(key.public())?;

        if self.removed.contains_key(&device.id) {
            return Err(MeshError::Removed(device.id));
        }

        self.author(key, OpBody::Add(device))
    }

    /// Signs a `Remove` as `key`, which must be a member desktop, and applies it.
    pub fn remove(&mut self, key: &SecretKey, id: EndpointId) -> Result<DeviceOp, MeshError> {
        self.gate(key.public())?;
        self.author(
            key,
            OpBody::Remove {
                id,
                at_ms: now_ms(),
            },
        )
    }

    fn author(&mut self, key: &SecretKey, body: OpBody) -> Result<DeviceOp, MeshError> {
        let op = DeviceOp::sign(key, self.mesh, body)?;
        let mut merged = self.merge(std::slice::from_ref(&op));
        if merged.rejected.is_empty() {
            Ok(op)
        } else {
            Err(merged.rejected.remove(0))
        }
    }

    /// Applies every op that counts, in any order. Ops whose author is not
    /// yet a member are held until it counts, the oldest dropped when the
    /// buffer is full. Refused ops are reported, never applied.
    pub fn merge(&mut self, ops: &[DeviceOp]) -> Merged {
        let before = self.member_ids();
        let mut merged = Merged::default();
        let mut items: Vec<(DeviceOp, bool)> =
            self.pending.drain(..).map(|op| (op, false)).collect();
        for op in ops {
            match op.verify(self.mesh) {
                Ok(()) => items.push((op.clone(), true)),
                Err(e) => merged.rejected.push(e),
            }
        }

        loop {
            let mut progress = false;
            let mut left = Vec::new();

            for (op, fresh) in items {
                match self.apply(&op) {
                    Ok(true) => progress = true,
                    Ok(false) => {}
                    Err(MeshError::Stranger(_)) => left.push((op, fresh)),
                    Err(e) if fresh => merged.rejected.push(e),
                    Err(_) => {}
                }
            }
            items = left;

            if !progress || items.is_empty() {
                break;
            }
        }

        for (op, fresh) in items {
            if self.pending.iter().any(|held| held.sig == op.sig) {
                continue;
            }

            if fresh {
                merged.held += 1;
            }
            if self.pending.len() >= MAX_PENDING {
                self.pending.remove(0);
            }

            self.pending.push(op);
        }

        let after = self.member_ids();
        merged.added = after.difference(&before).copied().collect();
        merged.removed = before.difference(&after).copied().collect();
        merged
    }

    fn member_ids(&self) -> BTreeSet<EndpointId> {
        self.members().iter().map(|d| d.id).collect()
    }

    /// Fails unless `author` is a member desktop.
    fn gate(&self, author: EndpointId) -> Result<(), MeshError> {
        if self.removed.contains_key(&author) {
            return Err(MeshError::Removed(author));
        }

        match self.added.get(&author) {
            None => Err(MeshError::Stranger(author)),
            Some(e) if e.device.kind != DeviceKind::Desktop => Err(MeshError::NotDesktop(author)),
            Some(_) => Ok(()),
        }
    }

    /// Ok(true) when the op counted now, Ok(false) when already held.
    fn apply(&mut self, op: &DeviceOp) -> Result<bool, MeshError> {
        if self.ops.iter().any(|o| o.sig == op.sig) {
            return Ok(false);
        }

        match &op.body {
            OpBody::Genesis(device) => {
                if op.author != self.mesh {
                    return Err(MeshError::OtherMesh);
                }

                if device.id != op.author || device.kind != DeviceKind::Desktop {
                    return Err(MeshError::NotDesktop(op.author));
                }
            }
            OpBody::Add(_) | OpBody::Remove { .. } => self.gate(op.author)?,
        }

        self.insert(op.clone());
        Ok(true)
    }

    fn insert(&mut self, op: DeviceOp) {
        match &op.body {
            OpBody::Genesis(device) | OpBody::Add(device) => {
                let entry = Entry {
                    device: device.clone(),
                    author: op.author,
                };
                let wins = match self.added.get(&device.id) {
                    None => true,
                    Some(old) => device.id != self.mesh && entry.rank() < old.rank(),
                };
                if wins {
                    self.added.insert(device.id, entry);
                }
            }
            OpBody::Remove { id, at_ms } => {
                let first = self.removed.entry(*id).or_insert(*at_ms);
                *first = (*first).min(*at_ms);
            }
        }

        self.ops.push(op);
    }
}

#[cfg(test)]
#[path = "../tests/devices.rs"]
mod tests;
