//! The device list: an append-only set of signed ops, `Genesis`, `Add` and
//! `Remove`. The mesh id is the genesis desktop's `EndpointId`.
//!
//! Every op is signed over the mesh id as well as its body, so an op signed
//! for one mesh cannot be replayed into another.
//!
//! An op counts if its author was added by a counted op and no `Remove` of the
//! author was known when the op arrived. Only a `Desktop` may author any op,
//! and a device's kind is whatever the signing desktop states in its `Add`.
//! When several `Add`s name one key, a `Desktop` beats a `Node`, then the
//! earlier `(added_at_ms, author)` wins; the genesis device never changes.
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
pub(crate) enum OpBody {
    Genesis(Device),
    Add(Device),
    Remove { id: EndpointId, at_ms: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceOp {
    pub author: EndpointId,
    body: OpBody,
    sig: Signature,
}

impl DeviceOp {
    fn message(mesh: EndpointId, body: &OpBody) -> Result<Vec<u8>, MeshError> {
        let mut msg = SIGN_DOMAIN.to_vec();
        msg.extend_from_slice(mesh.as_bytes());
        msg.extend(postcard::to_stdvec(body)?);
        Ok(msg)
    }

    pub(crate) fn sign(key: &SecretKey, mesh: EndpointId, body: OpBody) -> Result<Self, MeshError> {
        let sig = key.sign(&Self::message(mesh, &body)?);
        Ok(Self {
            author: key.public(),
            body,
            sig,
        })
    }

    /// A genesis is signed for the mesh its author starts, any other op for `mesh`.
    fn verify(&self, mesh: EndpointId) -> Result<(), MeshError> {
        let mesh = match self.body {
            OpBody::Genesis(_) => self.author,
            _ => mesh,
        };
        self.author
            .verify(&Self::message(mesh, &self.body)?, &self.sig)
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
    #[error("state file: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a state key: {0}")]
    BadKey(String),
    /// A replicated entry is stamped more than a day ahead of this clock.
    #[error("entry from {0} is stamped too far ahead")]
    Future(EndpointId),
    /// The write clock cannot move past what it has seen.
    #[error("write clock is exhausted")]
    Clock,
    #[error("record is too large")]
    TooLarge,
    /// A desktop declined a join. The text is meant for the person joining.
    #[error("refused: {0}")]
    Refused(String),
    #[error("this directory already belongs to a mesh")]
    AlreadyMember,
    #[error("network: {0}")]
    Net(String),
    #[error("the mesh is closed")]
    Closed,
    /// A stream to a node failed before it opened or while it was in use.
    #[error(transparent)]
    Link(anyhow::Error),
    /// The node said it will not serve the protocol.
    #[error("the node refused {protocol}: {reason}")]
    Declined { protocol: String, reason: String },
    /// The node answered an `Open` with something other than `Ready` or `Refused`.
    #[error("the node answered {0}")]
    Unexpected(String),
}

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
            .expect("postcard encodes a device op");
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
mod tests {
    use super::*;
    use iroh::SecretKey;

    fn key() -> SecretKey {
        SecretKey::generate()
    }

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

    fn from(ops: &[DeviceOp]) -> Devices {
        let (list, merged) = Devices::from_ops(ops).unwrap();
        assert!(merged.rejected.is_empty());
        list
    }

    fn only_genesis(src: &Devices) -> Devices {
        from(&src.ops()[..1])
    }

    fn kind_of(list: &Devices, id: EndpointId) -> DeviceKind {
        list.members().iter().find(|d| d.id == id).unwrap().kind
    }

    /// A validly signed op for `mesh` from a key nobody added.
    fn stranger_op(mesh: EndpointId) -> DeviceOp {
        let (s, x) = (key(), key());
        DeviceOp::sign(&s, mesh, OpBody::Add(device(&x, "x", DeviceKind::Node))).unwrap()
    }

    #[test]
    fn an_op_from_a_stranger_is_refused() {
        let (a, other) = (key(), key());
        let stranger = key();
        let mut mine = genesis(&a);
        let op = DeviceOp::sign(
            &stranger,
            mine.mesh(),
            OpBody::Add(device(&other, "x", DeviceKind::Node)),
        )
        .unwrap();
        let merged = mine.merge(&[op]);
        assert_eq!(merged.held, 1);
        assert!(merged.added.is_empty());
        assert!(!mine.is_member(other.public()));
    }

    #[test]
    fn an_op_with_a_bad_signature_is_refused() {
        let (a, b, c) = (key(), key(), key());
        let mut mine = genesis(&a);
        let mut theirs = mine.clone();
        let mut op = theirs.add(&a, device(&b, "b", DeviceKind::Node)).unwrap();
        // Same op, but the device it names is swapped after signing.
        let forged = theirs.add(&a, device(&c, "c", DeviceKind::Node)).unwrap();
        op.body = forged.body;
        let merged = mine.merge(&[op]);
        assert!(matches!(
            merged.rejected.as_slice(),
            [MeshError::BadSignature]
        ));
        assert!(!mine.is_member(b.public()) && !mine.is_member(c.public()));
    }

    #[test]
    fn a_good_remove_is_reported_next_to_junk_ops() {
        let (a, b) = (key(), key());
        let mut mine = genesis(&a);
        mine.add(&a, device(&b, "b", DeviceKind::Node)).unwrap();
        let mut src = mine.clone();
        let remove = src.remove(&a, b.public()).unwrap();
        let mut bad = stranger_op(a.public());
        bad.author = a.public();
        let merged = mine.merge(&[bad, stranger_op(a.public()), remove]);
        assert_eq!(merged.removed, vec![b.public()]);
        assert_eq!(merged.held, 1);
        assert!(matches!(
            merged.rejected.as_slice(),
            [MeshError::BadSignature]
        ));
        assert!(!mine.is_member(b.public()));
    }

    #[test]
    fn a_flooded_buffer_still_takes_a_legitimate_out_of_order_op() {
        let (a, b, c) = (key(), key(), key());
        let mut src = genesis(&a);
        let op_b = src.add(&a, device(&b, "b", DeviceKind::Desktop)).unwrap();
        let op_c = src.add(&b, device(&c, "c", DeviceKind::Node)).unwrap();
        let mut mine = only_genesis(&src);
        for _ in 0..(MAX_PENDING + 50) {
            mine.merge(&[stranger_op(a.public())]);
        }
        let merged = mine.merge(&[op_c]);
        assert_eq!(merged.held, 1);
        let merged = mine.merge(&[op_b]);
        assert_eq!(merged.added, {
            let mut ids = vec![b.public(), c.public()];
            ids.sort();
            ids
        });
        assert!(mine.is_member(c.public()));
    }

    #[test]
    fn a_repeated_stranger_op_is_held_once() {
        let a = key();
        let mut mine = genesis(&a);
        let op = stranger_op(a.public());
        for _ in 0..5 {
            mine.merge(std::slice::from_ref(&op));
        }
        assert_eq!(mine.pending.len(), 1);
    }

    #[test]
    fn a_remove_signed_for_one_mesh_does_not_replay_into_another() {
        let (a, c, b, victim) = (key(), key(), key(), key());
        let mut x = genesis(&a);
        let mut y = genesis(&c);
        for list in [&mut x, &mut y] {
            let owner = if list.mesh() == a.public() { &a } else { &c };
            list.add(owner, device(&b, "b", DeviceKind::Desktop))
                .unwrap();
            list.add(owner, device(&victim, "v", DeviceKind::Node))
                .unwrap();
        }
        let op = x.remove(&b, victim.public()).unwrap();
        let merged = y.merge(&[op]);
        assert!(matches!(
            merged.rejected.as_slice(),
            [MeshError::BadSignature]
        ));
        assert!(y.is_member(victim.public()));
    }

    #[test]
    fn a_member_adds_a_device_and_another_member_sees_it() {
        let (a, b, c) = (key(), key(), key());
        let mut one = genesis(&a);
        let op = one.add(&a, device(&b, "b", DeviceKind::Desktop)).unwrap();
        let mut two = from(one.ops());
        assert!(two.is_member(b.public()));
        let op2 = two.add(&b, device(&c, "c", DeviceKind::Node)).unwrap();
        let merged = one.merge(&[op2]);
        assert_eq!(merged.added, vec![c.public()]);
        assert!(one.is_member(c.public()));
        assert_eq!(one.mesh(), a.public());
        assert_eq!(op.author, a.public());
        assert_eq!(one.members().len(), 3);
    }

    #[test]
    fn merging_ops_in_any_order_gives_the_same_members() {
        let keys: Vec<SecretKey> = (0..4).map(|_| key()).collect();
        let mut src = genesis(&keys[0]);
        let mut all = vec![
            src.add(&keys[0], device(&keys[1], "b", DeviceKind::Desktop))
                .unwrap(),
            src.add(&keys[0], device(&keys[2], "c", DeviceKind::Node))
                .unwrap(),
        ];
        let mut second = src.clone();
        all.push(
            second
                .add(&keys[1], device(&keys[3], "d", DeviceKind::Node))
                .unwrap(),
        );
        let expected = {
            let mut d = only_genesis(&src);
            d.merge(&all);
            d.members()
        };
        assert_eq!(expected.len(), 4);
        for order in permutations(&all) {
            let mut d = only_genesis(&src);
            let merged = d.merge(&order);
            assert!(merged.rejected.is_empty() && merged.held == 0);
            assert_eq!(d.members(), expected);
        }
    }

    #[test]
    fn a_low_timestamp_node_readd_leaves_a_desktop_in_every_order() {
        let (a, b) = (key(), key());
        let mut src = genesis(&a);
        let desktop = Device {
            added_at_ms: 100,
            ..device(&b, "b", DeviceKind::Desktop)
        };
        let node = Device {
            added_at_ms: 1,
            ..device(&b, "b", DeviceKind::Node)
        };
        let genesis_again = Device {
            added_at_ms: 0,
            ..device(&a, "a", DeviceKind::Node)
        };
        let ops = vec![
            src.add(&a, desktop).unwrap(),
            src.add(&a, node).unwrap(),
            src.add(&a, genesis_again).unwrap(),
        ];
        for order in permutations(&ops) {
            let mut d = only_genesis(&src);
            d.merge(&order);
            assert_eq!(kind_of(&d, b.public()), DeviceKind::Desktop);
            assert_eq!(kind_of(&d, a.public()), DeviceKind::Desktop);
        }
    }

    #[test]
    fn a_removed_key_stays_out_when_added_again() {
        let (a, b) = (key(), key());
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
        let (a, b, c) = (key(), key(), key());
        let mut d = genesis(&a);
        d.add(&a, device(&b, "b", DeviceKind::Desktop)).unwrap();
        let mut b_side = d.clone();
        let late_add = b_side.add(&b, device(&c, "c", DeviceKind::Node)).unwrap();
        d.remove(&a, b.public()).unwrap();
        let merged = d.merge(&[late_add]);
        assert!(matches!(
            merged.rejected.as_slice(),
            [MeshError::Removed(_)]
        ));
        assert!(!d.is_member(c.public()));
        assert!(matches!(
            d.add(&b, device(&c, "c", DeviceKind::Node)),
            Err(MeshError::Removed(_))
        ));
    }

    /// The accepted limit from the module comment: a removed desktop's Add
    /// stays when a member saw it before the Remove, and is refused after.
    #[test]
    fn a_removed_desktops_add_stays_only_if_it_arrived_before_the_remove() {
        let (a, b, c) = (key(), key(), key());
        let mut base = genesis(&a);
        base.add(&a, device(&b, "b", DeviceKind::Desktop)).unwrap();
        let late = base
            .clone()
            .add(&b, device(&c, "c", DeviceKind::Node))
            .unwrap();
        let remove = base.clone().remove(&a, b.public()).unwrap();

        let mut before = base.clone();
        before.merge(std::slice::from_ref(&late));
        before.merge(std::slice::from_ref(&remove));
        assert!(before.is_member(c.public()));

        let mut after = base.clone();
        after.merge(std::slice::from_ref(&remove));
        after.merge(std::slice::from_ref(&late));
        assert!(!after.is_member(c.public()));
    }

    #[test]
    fn a_list_from_another_mesh_is_refused() {
        let (a, b) = (key(), key());
        let other = genesis(&b);
        let mut mine = genesis(&a);
        let merged = mine.merge(other.ops());
        assert!(matches!(merged.rejected.as_slice(), [MeshError::OtherMesh]));
        assert_eq!(mine.members().len(), 1);
    }

    #[test]
    fn a_node_cannot_add_a_device() {
        let (a, n, x) = (key(), key(), key());
        let mut d = genesis(&a);
        d.add(&a, device(&n, "n", DeviceKind::Node)).unwrap();
        let mut node_side = d.clone();
        assert!(matches!(
            node_side.add(&n, device(&x, "x", DeviceKind::Desktop)),
            Err(MeshError::NotDesktop(_))
        ));
        // A node that signs one anyway is refused by the receiver.
        let forged = DeviceOp::sign(
            &n,
            d.mesh(),
            OpBody::Add(device(&x, "x", DeviceKind::Desktop)),
        )
        .unwrap();
        let merged = d.merge(&[forged]);
        assert!(matches!(
            merged.rejected.as_slice(),
            [MeshError::NotDesktop(_)]
        ));
        assert!(!d.is_member(x.public()));
    }

    #[test]
    fn a_node_cannot_remove_a_device() {
        let (a, n, m) = (key(), key(), key());
        let mut d = genesis(&a);
        d.add(&a, device(&n, "n", DeviceKind::Node)).unwrap();
        d.add(&a, device(&m, "m", DeviceKind::Node)).unwrap();
        assert!(matches!(
            d.remove(&n, m.public()),
            Err(MeshError::NotDesktop(_))
        ));
        let forged = DeviceOp::sign(
            &n,
            d.mesh(),
            OpBody::Remove {
                id: m.public(),
                at_ms: 1,
            },
        )
        .unwrap();
        let merged = d.merge(&[forged]);
        assert!(matches!(
            merged.rejected.as_slice(),
            [MeshError::NotDesktop(_)]
        ));
        assert!(d.is_member(m.public()));
    }

    #[test]
    fn a_genesis_from_a_node_is_refused() {
        let n = key();
        let d = Devices::genesis(&n, "n", DeviceKind::Node);
        assert!(matches!(
            Devices::from_ops(d.ops()),
            Err(MeshError::NotDesktop(_))
        ));
    }

    #[test]
    fn from_ops_keeps_the_good_ops_and_reports_the_refused_ones() {
        let (a, b) = (key(), key());
        let mut d = genesis(&a);
        d.add(&a, device(&b, "b", DeviceKind::Node)).unwrap();
        let mut ops = d.ops().to_vec();
        ops.push(genesis(&key()).ops()[0].clone());
        ops.push(stranger_op(a.public()));
        let (list, merged) = Devices::from_ops(&ops).unwrap();
        assert_eq!(list.mesh(), a.public());
        assert!(list.is_member(b.public()));
        assert!(matches!(merged.rejected.as_slice(), [MeshError::OtherMesh]));
        assert_eq!(merged.held, 1);
        assert!(matches!(
            Devices::from_ops(&[stranger_op(a.public())]),
            Err(MeshError::NoGenesis)
        ));
    }

    #[test]
    fn write_limit_follows_kind_and_removal() {
        let (a, d, n, x) = (key(), key(), key(), key());
        let mut list = genesis(&a);
        list.add(&a, device(&d, "d", DeviceKind::Desktop)).unwrap();
        list.add(&a, device(&n, "n", DeviceKind::Node)).unwrap();
        assert_eq!(list.write_limit(&a.public()), WriteLimit::Always);
        assert_eq!(list.write_limit(&d.public()), WriteLimit::Always);
        assert_eq!(list.write_limit(&n.public()), WriteLimit::Refused);
        assert_eq!(list.write_limit(&x.public()), WriteLimit::Refused);
        list.remove(&a, d.public()).unwrap();
        list.remove(&a, n.public()).unwrap();
        assert!(matches!(
            list.write_limit(&d.public()),
            WriteLimit::Until(_)
        ));
        assert_eq!(list.write_limit(&n.public()), WriteLimit::Refused);
    }
}
