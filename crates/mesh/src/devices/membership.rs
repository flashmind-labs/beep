//! Membership and authorization over signed operations.

use super::{Device, DeviceKind, DeviceOp, OpBody};
use crate::clock::now_ms;
use crate::error::MeshError;
use iroh::{EndpointId, SecretKey};
use std::collections::BTreeMap;

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
    /// Ops of this call that were refused, and why.
    pub rejected: Vec<MeshError>,
}

/// A device and the op that put it in the list. When two desktops add the
/// same key, the earlier `(added_at_ms, author)` wins, whatever the arrival order.
#[derive(Clone, Debug)]
struct Entry {
    device: Device,
    author: EndpointId,
    genesis: bool,
}

impl Entry {
    /// Smaller wins: the genesis, then a desktop beats a node, then the earlier add.
    fn rank(&self) -> (bool, bool, u64, EndpointId) {
        (
            !self.genesis,
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
    /// Every `Genesis` and `Add` naming a key.
    added: BTreeMap<EndpointId, Vec<Entry>>,
    /// Removed key and the latest `at_ms` of a `Remove` naming it. An `Add`
    /// stamped later brings the key back.
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
    /// valid first genesis from a desktop; the ops that were refused come
    /// back in the `Merged`, and the ones from strangers stay held. Compare `mesh()` with the ticket's.
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
        self.current(&id).is_some()
    }

    /// A key that was removed and not added again since.
    pub fn is_removed(&self, id: EndpointId) -> bool {
        self.removed.contains_key(&id) && !self.is_member(id)
    }

    pub fn is_desktop(&self, id: EndpointId) -> bool {
        self.current(&id)
            .is_some_and(|e| e.device.kind == DeviceKind::Desktop)
    }

    /// Which entries from `id` may still be taken.
    pub fn write_limit(&self, id: &EndpointId) -> WriteLimit {
        if let Some(entry) = self.current(id) {
            return match entry.device.kind {
                DeviceKind::Desktop => WriteLimit::Always,
                DeviceKind::Node => WriteLimit::Refused,
            };
        }

        let was_desktop = self
            .added
            .get(id)
            .is_some_and(|entries| entries.iter().any(|e| e.device.kind == DeviceKind::Desktop));

        match self.removed.get(id) {
            Some(at_ms) if was_desktop => WriteLimit::Until(*at_ms),
            _ => WriteLimit::Refused,
        }
    }

    pub fn members(&self) -> Vec<Device> {
        self.added
            .keys()
            .filter_map(|id| self.current(id))
            .map(|entry| entry.device.clone())
            .collect()
    }

    /// Signs an `Add` as `key`, which must be a member desktop, and applies
    /// it. A removed key comes back: the `Add` is stamped after its last
    /// `Remove`, whatever this clock says.
    pub fn add(&mut self, key: &SecretKey, mut device: Device) -> Result<DeviceOp, MeshError> {
        self.gate(key.public())?;

        if let Some(at_ms) = self.removed.get(&device.id) {
            device.added_at_ms = device.added_at_ms.max(at_ms.saturating_add(1));
        }

        self.author(key, OpBody::Add(device))
    }

    /// Signs a `Remove` as `key`, which must be a member desktop, and applies
    /// it, stamped after every `Add` of `id` so it counts whatever this clock says.
    pub fn remove(&mut self, key: &SecretKey, id: EndpointId) -> Result<DeviceOp, MeshError> {
        self.gate(key.public())?;
        let last_add = self
            .added
            .get(&id)
            .into_iter()
            .flatten()
            .map(|e| e.device.added_at_ms.saturating_add(1))
            .max()
            .unwrap_or(0);
        self.author(
            key,
            OpBody::Remove {
                id,
                at_ms: now_ms().max(last_add),
            },
        )
    }

    /// The entry that makes `id` a member: the best `Add` stamped after its
    /// last `Remove`.
    fn current(&self, id: &EndpointId) -> Option<&Entry> {
        let removed_at = self.removed.get(id);
        self.added
            .get(id)?
            .iter()
            .filter(|e| removed_at.is_none_or(|at| e.device.added_at_ms > *at))
            .min_by_key(|e| e.rank())
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

        for (op, _) in items {
            if self.pending.iter().any(|held| held.sig == op.sig) {
                continue;
            }

            if self.pending.len() >= MAX_PENDING {
                self.pending.remove(0);
            }

            self.pending.push(op);
        }

        merged
    }

    /// Fails unless `author` is a member desktop.
    fn gate(&self, author: EndpointId) -> Result<(), MeshError> {
        match self.current(&author) {
            None if self.removed.contains_key(&author) => Err(MeshError::Removed(author)),
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
                self.added.entry(device.id).or_default().push(Entry {
                    device: device.clone(),
                    author: op.author,
                    genesis: matches!(op.body, OpBody::Genesis(_)),
                });
            }
            OpBody::Remove { id, at_ms } => {
                let last = self.removed.entry(*id).or_insert(*at_ms);
                *last = (*last).max(*at_ms);
            }
        }

        self.ops.push(op);
    }
}

#[cfg(test)]
#[path = "../tests/devices.rs"]
mod tests;
