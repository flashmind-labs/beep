//! The device list: an append-only set of signed ops, `Genesis`, `Add` and
//! `Remove`. The mesh id is the genesis desktop's `EndpointId`.
//!
//! An op counts if its author was added by a counted op and no `Remove` of the
//! author was known when the op arrived. Only a `Desktop` may author any op,
//! and a device's kind is whatever the signing desktop states in its `Add`.
//! A removal is permanent: that key never rejoins, it joins with a new key.
//!
//! Accepted limit: an `Add` signed by a removed device, seen by a member
//! before the `Remove`, stays. Remove that device too.

use iroh::{EndpointId, SecretKey, Signature};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};
use strum::{Display, IntoStaticStr};
use thiserror::Error;

const SIGN_DOMAIN: &[u8] = b"beep-mesh/1 device-op";
/// Ops held while waiting for their author to count.
const MAX_PENDING: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
pub enum DeviceKind {
    Node,
    Desktop,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    pub id: EndpointId,
    pub name: String,
    pub kind: DeviceKind,
    pub added_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum OpBody {
    Genesis(Device),
    Add(Device),
    Remove(EndpointId),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceOp {
    pub author: EndpointId,
    body: OpBody,
    sig: Signature,
}

impl DeviceOp {
    fn message(body: &OpBody) -> Result<Vec<u8>, MeshError> {
        let mut msg = SIGN_DOMAIN.to_vec();
        msg.extend(postcard::to_stdvec(body)?);
        Ok(msg)
    }

    fn sign(key: &SecretKey, body: OpBody) -> Result<Self, MeshError> {
        let sig = key.sign(&Self::message(&body)?);
        Ok(Self {
            author: key.public(),
            body,
            sig,
        })
    }

    fn verify(&self) -> Result<(), MeshError> {
        self.author
            .verify(&Self::message(&self.body)?, &self.sig)
            .map_err(|_| MeshError::BadSignature)
    }
}

#[derive(Debug, Error)]
pub enum MeshError {
    /// The author is not (yet) a member.
    #[error("{0} is not a member")]
    Stranger(EndpointId),
    /// The key was removed and cannot author or rejoin.
    #[error("{0} was removed")]
    Removed(EndpointId),
    #[error("{0} is not a desktop")]
    NotDesktop(EndpointId),
    #[error("bad signature")]
    BadSignature,
    #[error("op belongs to another mesh")]
    OtherMesh,
    #[error("no genesis op")]
    NoGenesis,
    #[error("encoding failed: {0}")]
    Encode(#[from] postcard::Error),
}

/// What a merge changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Merged {
    pub added: Vec<EndpointId>,
    pub removed: Vec<EndpointId>,
}

/// A device and the op that put it in the list. When two desktops add the
/// same key, the earlier `(added_at_ms, author)` wins, whatever the arrival order.
#[derive(Clone, Debug)]
struct Entry {
    device: Device,
    author: EndpointId,
}

#[derive(Clone, Debug)]
pub struct Devices {
    mesh: EndpointId,
    ops: Vec<DeviceOp>,
    pending: Vec<DeviceOp>,
    added: BTreeMap<EndpointId, Entry>,
    removed: BTreeSet<EndpointId>,
}

fn now_ms() -> u64 {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    u64::try_from(since).unwrap_or(u64::MAX)
}

impl Devices {
    fn empty(mesh: EndpointId) -> Self {
        Self {
            mesh,
            ops: Vec::new(),
            pending: Vec::new(),
            added: BTreeMap::new(),
            removed: BTreeSet::new(),
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
        // The body is plain data that postcard always encodes.
        if let Ok(op) = DeviceOp::sign(key, OpBody::Genesis(device)) {
            list.insert(op);
        }
        list
    }

    /// Builds a list from ops received from a peer, genesis included.
    pub fn from_ops(ops: &[DeviceOp]) -> Result<Devices, MeshError> {
        let genesis = ops
            .iter()
            .find(|op| matches!(op.body, OpBody::Genesis(_)))
            .ok_or(MeshError::NoGenesis)?;
        let mut list = Self::empty(genesis.author);
        list.merge(ops)?;
        Ok(list)
    }

    pub fn mesh(&self) -> EndpointId {
        self.mesh
    }

    pub fn ops(&self) -> &[DeviceOp] {
        &self.ops
    }

    pub fn is_member(&self, id: EndpointId) -> bool {
        self.added.contains_key(&id) && !self.removed.contains(&id)
    }

    pub fn members(&self) -> Vec<Device> {
        self.added
            .iter()
            .filter(|(id, _)| !self.removed.contains(*id))
            .map(|(_, entry)| entry.device.clone())
            .collect()
    }

    /// Signs an `Add` as `key`, which must be a member desktop, and applies it.
    pub fn add(&mut self, key: &SecretKey, device: Device) -> Result<DeviceOp, MeshError> {
        self.gate(key.public())?;
        if self.removed.contains(&device.id) {
            return Err(MeshError::Removed(device.id));
        }
        self.author(key, OpBody::Add(device))
    }

    /// Signs a `Remove` as `key`, which must be a member desktop, and applies it.
    pub fn remove(&mut self, key: &SecretKey, id: EndpointId) -> Result<DeviceOp, MeshError> {
        self.gate(key.public())?;
        self.author(key, OpBody::Remove(id))
    }

    fn author(&mut self, key: &SecretKey, body: OpBody) -> Result<DeviceOp, MeshError> {
        let op = DeviceOp::sign(key, body)?;
        self.merge(std::slice::from_ref(&op))?;
        Ok(op)
    }

    /// Applies every op that counts, in any order. Ops whose author is not
    /// yet a member are held until it counts. When any op of the call was
    /// refused or is still held, the first such error is returned, and the
    /// ops that did count stay applied.
    pub fn merge(&mut self, ops: &[DeviceOp]) -> Result<Merged, MeshError> {
        let before: BTreeSet<EndpointId> = self.members().iter().map(|d| d.id).collect();
        let mut first_err = None;
        let mut items: Vec<(DeviceOp, bool)> =
            self.pending.drain(..).map(|op| (op, false)).collect();
        for op in ops {
            match op.verify() {
                Ok(()) => items.push((op.clone(), true)),
                Err(e) => {
                    first_err.get_or_insert(e);
                }
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
                    Err(e) if fresh => {
                        first_err.get_or_insert(e);
                    }
                    Err(_) => {}
                }
            }
            items = left;
            if !progress || items.is_empty() {
                break;
            }
        }
        for (op, fresh) in items {
            if fresh {
                first_err.get_or_insert(MeshError::Stranger(op.author));
            }
            if self.pending.len() < MAX_PENDING {
                self.pending.push(op);
            }
        }
        if let Some(e) = first_err {
            return Err(e);
        }
        let after: BTreeSet<EndpointId> = self.members().iter().map(|d| d.id).collect();
        Ok(Merged {
            added: after.difference(&before).copied().collect(),
            removed: before.difference(&after).copied().collect(),
        })
    }

    /// Fails unless `author` is a member desktop.
    fn gate(&self, author: EndpointId) -> Result<(), MeshError> {
        if self.removed.contains(&author) {
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
            OpBody::Add(_) | OpBody::Remove(_) => self.gate(op.author)?,
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
                let wins = self.added.get(&device.id).is_none_or(|old| {
                    (entry.device.added_at_ms, entry.author) < (old.device.added_at_ms, old.author)
                });
                if wins {
                    self.added.insert(device.id, entry);
                }
            }
            OpBody::Remove(id) => {
                self.removed.insert(*id);
            }
        }
        self.ops.push(op);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    fn device(key: &SecretKey, name: &str, kind: DeviceKind) -> Device {
        Device {
            id: key.public(),
            name: name.to_owned(),
            kind,
            added_at_ms: 1,
        }
    }

    fn permutations(ops: &[DeviceOp]) -> Vec<Vec<DeviceOp>> {
        if ops.len() <= 1 {
            return vec![ops.to_vec()];
        }
        let mut all = Vec::new();
        for i in 0..ops.len() {
            let mut rest = ops.to_vec();
            let first = rest.remove(i);
            for mut tail in permutations(&rest) {
                tail.insert(0, first.clone());
                all.push(tail);
            }
        }
        all
    }

    fn genesis(key: &SecretKey) -> Devices {
        Devices::genesis(key, "first", DeviceKind::Desktop)
    }

    #[test]
    fn an_op_from_a_stranger_is_refused() {
        let (a, stranger, other) = (
            SecretKey::generate(),
            SecretKey::generate(),
            SecretKey::generate(),
        );
        let mut theirs = genesis(&stranger);
        let op = theirs
            .add(&stranger, device(&other, "x", DeviceKind::Node))
            .unwrap();
        let mut mine = genesis(&a);
        assert!(matches!(mine.merge(&[op]), Err(MeshError::Stranger(_))));
        assert!(!mine.is_member(other.public()));
    }

    #[test]
    fn an_op_with_a_bad_signature_is_refused() {
        let (a, b, c) = (
            SecretKey::generate(),
            SecretKey::generate(),
            SecretKey::generate(),
        );
        let mut mine = genesis(&a);
        let mut theirs = mine.clone();
        let mut op = theirs.add(&a, device(&b, "b", DeviceKind::Node)).unwrap();
        // Same op, but the device it names is swapped after signing.
        let forged = theirs.add(&a, device(&c, "c", DeviceKind::Node)).unwrap();
        op.body = forged.body;
        assert!(matches!(mine.merge(&[op]), Err(MeshError::BadSignature)));
        assert!(!mine.is_member(b.public()) && !mine.is_member(c.public()));
    }

    #[test]
    fn a_member_adds_a_device_and_another_member_sees_it() {
        let (a, b, c) = (
            SecretKey::generate(),
            SecretKey::generate(),
            SecretKey::generate(),
        );
        let mut one = genesis(&a);
        let op = one.add(&a, device(&b, "b", DeviceKind::Desktop)).unwrap();
        let mut two = Devices::from_ops(one.ops()).unwrap();
        assert!(two.is_member(b.public()));
        let op2 = two.add(&b, device(&c, "c", DeviceKind::Node)).unwrap();
        let merged = one.merge(&[op2]).unwrap();
        assert_eq!(merged.added, vec![c.public()]);
        assert!(one.is_member(c.public()));
        assert_eq!(one.mesh(), a.public());
        assert_eq!(op.author, a.public());
        assert_eq!(one.members().len(), 3);
    }

    #[test]
    fn merging_ops_in_any_order_gives_the_same_members() {
        let keys: Vec<SecretKey> = (0..4).map(|_| SecretKey::generate()).collect();
        let mut src = genesis(&keys[0]);
        let ops = vec![
            src.add(&keys[0], device(&keys[1], "b", DeviceKind::Desktop))
                .unwrap(),
            src.add(&keys[0], device(&keys[2], "c", DeviceKind::Node))
                .unwrap(),
        ];
        let mut second = src.clone();
        let late = second
            .add(&keys[1], device(&keys[3], "d", DeviceKind::Node))
            .unwrap();
        let mut all = ops;
        all.push(late);
        let expected = {
            let mut d = Devices::from_ops(src.ops()).unwrap();
            d.merge(&all).unwrap();
            d.members()
        };
        assert_eq!(expected.len(), 4);
        for order in permutations(&all) {
            let mut d = genesis_only(&src);
            d.merge(&order).unwrap();
            assert_eq!(d.members(), expected);
        }
    }

    fn genesis_only(src: &Devices) -> Devices {
        Devices::from_ops(&src.ops()[..1]).unwrap()
    }

    #[test]
    fn a_removed_key_stays_out_when_added_again() {
        let (a, b) = (SecretKey::generate(), SecretKey::generate());
        let mut d = genesis(&a);
        d.add(&a, device(&b, "b", DeviceKind::Node)).unwrap();
        d.remove(&a, b.public()).unwrap();
        assert!(!d.is_member(b.public()));
        let again = d.add(&a, device(&b, "b", DeviceKind::Node));
        assert!(matches!(again, Err(MeshError::Removed(_))));
        assert!(!d.is_member(b.public()));
    }

    #[test]
    fn a_removed_device_cannot_add_once_its_removal_is_known() {
        let (a, b, c) = (
            SecretKey::generate(),
            SecretKey::generate(),
            SecretKey::generate(),
        );
        let mut d = genesis(&a);
        d.add(&a, device(&b, "b", DeviceKind::Desktop)).unwrap();
        let mut b_side = d.clone();
        let late_add = b_side.add(&b, device(&c, "c", DeviceKind::Node)).unwrap();
        d.remove(&a, b.public()).unwrap();
        assert!(d.merge(&[late_add]).is_err());
        assert!(!d.is_member(c.public()));
        assert!(matches!(
            d.add(&b, device(&c, "c", DeviceKind::Node)),
            Err(MeshError::Removed(_))
        ));
    }

    #[test]
    fn a_list_from_another_mesh_is_refused() {
        let (a, b) = (SecretKey::generate(), SecretKey::generate());
        let other = genesis(&b);
        let mut mine = genesis(&a);
        assert!(matches!(mine.merge(other.ops()), Err(MeshError::OtherMesh)));
        assert_eq!(mine.members().len(), 1);
    }

    #[test]
    fn a_node_cannot_add_a_device() {
        let (a, n, x) = (
            SecretKey::generate(),
            SecretKey::generate(),
            SecretKey::generate(),
        );
        let mut d = genesis(&a);
        d.add(&a, device(&n, "n", DeviceKind::Node)).unwrap();
        let mut node_side = d.clone();
        assert!(matches!(
            node_side.add(&n, device(&x, "x", DeviceKind::Desktop)),
            Err(MeshError::NotDesktop(_))
        ));
        // A node that signs one anyway is refused by the receiver.
        let forged = DeviceOp::sign(&n, OpBody::Add(device(&x, "x", DeviceKind::Desktop))).unwrap();
        assert!(matches!(d.merge(&[forged]), Err(MeshError::NotDesktop(_))));
        assert!(!d.is_member(x.public()));
    }

    #[test]
    fn a_node_cannot_remove_a_device() {
        let (a, n, m) = (
            SecretKey::generate(),
            SecretKey::generate(),
            SecretKey::generate(),
        );
        let mut d = genesis(&a);
        d.add(&a, device(&n, "n", DeviceKind::Node)).unwrap();
        d.add(&a, device(&m, "m", DeviceKind::Node)).unwrap();
        assert!(matches!(
            d.remove(&n, m.public()),
            Err(MeshError::NotDesktop(_))
        ));
        let forged = DeviceOp::sign(&n, OpBody::Remove(m.public())).unwrap();
        assert!(matches!(d.merge(&[forged]), Err(MeshError::NotDesktop(_))));
        assert!(d.is_member(m.public()));
    }

    #[test]
    fn a_genesis_from_a_node_is_refused() {
        let n = SecretKey::generate();
        let d = Devices::genesis(&n, "n", DeviceKind::Node);
        assert!(matches!(
            Devices::from_ops(d.ops()),
            Err(MeshError::NotDesktop(_))
        ));
    }
}
