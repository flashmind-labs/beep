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
    mine.merge(&[op]);
    assert_eq!(mine.pending.len(), 1);
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
    assert_eq!(mine.pending.len(), 1);
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

    mine.merge(&[op_c.clone()]);
    assert!(mine.pending.iter().any(|held| held.sig == op_c.sig));
    mine.merge(&[op_b]);
    assert!(mine.is_member(b.public()));
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
    one.merge(&[op2]);
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
        assert!(merged.rejected.is_empty() && d.pending.is_empty());
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
fn a_removed_key_comes_back_when_added_again_in_any_order() {
    let (a, b) = (key(), key());
    let mut src = genesis(&a);
    let ops = vec![
        src.add(&a, device(&b, "b", DeviceKind::Node)).unwrap(),
        src.remove(&a, b.public()).unwrap(),
        src.add(&a, device(&b, "b", DeviceKind::Node)).unwrap(),
    ];
    assert!(src.is_member(b.public()));

    for order in permutations(&ops) {
        let mut d = only_genesis(&src);
        d.merge(&order);
        assert!(d.is_member(b.public()));
        assert!(!d.is_removed(b.public()));
    }

    let gone = src.remove(&a, b.public()).unwrap();
    let mut all = ops;
    all.push(gone);

    for order in permutations(&all) {
        let mut d = only_genesis(&src);
        d.merge(&order);
        assert!(d.is_removed(b.public()));
    }
}

/// A clock behind the last op still orders a new `Add` or `Remove` after it.
#[test]
fn a_slow_clock_cannot_undo_a_remove_or_an_add() {
    let (a, b) = (key(), key());
    let mut d = genesis(&a);
    let future = now_ms() + 3_600_000;
    let ahead = Device {
        added_at_ms: future,
        ..device(&b, "b", DeviceKind::Node)
    };
    d.add(&a, ahead).unwrap();
    d.remove(&a, b.public()).unwrap();
    assert!(d.is_removed(b.public()));
    d.add(&a, device(&b, "b", DeviceKind::Node)).unwrap();
    assert!(d.is_member(b.public()));
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
    assert_eq!(list.pending.len(), 1);
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
