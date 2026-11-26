use super::*;
use iroh::SecretKey;
use std::sync::atomic::{AtomicBool, Ordering};

/// A mesh that lists nobody and removes nobody.
struct Alone;

impl Access for Alone {
    fn may_connect(&self, _: &EndpointId) -> bool {
        false
    }
    fn may_get(&self, _: &EndpointId, _: &Hash) -> bool {
        false
    }
}

#[test]
fn only_the_granted_peer_gets_the_granted_root() {
    let grants = Arc::new(Grants::new(Arc::new(Alone)));
    let peer = SecretKey::generate().public();
    let stranger = SecretKey::generate().public();
    let file = Hash::new(b"file");
    let folder = Hash::new(b"folder");
    let guard = grants.grant(peer, HashAndFormat::raw(file));
    let folder_guard = grants.grant(peer, HashAndFormat::hash_seq(folder));

    assert!(grants.connect(1, Some(stranger)).is_err());
    assert!(grants.connect(2, None).is_err());
    assert!(grants.connect(3, Some(peer)).is_ok());
    assert!(grants.get(3, &GetRequest::blob(file)).is_ok());
    assert!(grants.get(3, &GetRequest::all(folder)).is_ok());
    // A file's hash read as a folder would reach other blobs.
    assert!(grants.get(3, &GetRequest::all(file)).is_err());
    assert!(
        grants
            .get(3, &GetRequest::blob(Hash::new(b"other")))
            .is_err()
    );
    assert!(grants.get(1, &GetRequest::blob(file)).is_err());

    drop(guard);
    assert!(grants.get(3, &GetRequest::blob(file)).is_err());
    drop(folder_guard);
    assert!(grants.connect(4, Some(peer)).is_err());
    grants.closed(3);
    assert!(grants.get(3, &GetRequest::all(folder)).is_err());
}

struct Member(EndpointId, Hash);

impl Access for Member {
    fn may_connect(&self, peer: &EndpointId) -> bool {
        *peer == self.0
    }
    fn may_get(&self, peer: &EndpointId, hash: &Hash) -> bool {
        *peer == self.0 && *hash == self.1
    }
}

/// A mesh from which `gone` is removed once `removed` is set.
struct Removed {
    gone: EndpointId,
    removed: AtomicBool,
}

impl Removed {
    fn new(gone: EndpointId, removed: bool) -> Arc<Self> {
        Arc::new(Self {
            gone,
            removed: AtomicBool::new(removed),
        })
    }
}

impl Access for Removed {
    fn allows(&self, peer: &EndpointId) -> bool {
        *peer != self.gone || !self.removed.load(Ordering::SeqCst)
    }
    fn may_connect(&self, _: &EndpointId) -> bool {
        false
    }
    fn may_get(&self, _: &EndpointId, _: &Hash) -> bool {
        false
    }
}

#[test]
fn a_removed_device_cannot_fetch_an_open_offer() {
    let gone = SecretKey::generate().public();
    let mesh = Removed::new(gone, false);
    let grants = Arc::new(Grants::new(mesh.clone()));
    let kept = SecretKey::generate().public();
    let file = Hash::new(b"offered");
    let _gone_guard = grants.grant(gone, HashAndFormat::raw(file));
    let _kept_guard = grants.grant(kept, HashAndFormat::raw(file));
    // Admitted before the removal, then removed while connected.
    assert!(grants.connect(1, Some(gone)).is_ok());
    mesh.removed.store(true, Ordering::SeqCst);
    assert!(grants.get(1, &GetRequest::blob(file)).is_err());
    assert!(grants.connect(2, Some(gone)).is_err());
    assert!(grants.connect(3, Some(kept)).is_ok());
    assert!(grants.get(3, &GetRequest::blob(file)).is_ok());
}

#[test]
fn a_device_outside_the_mesh_keeps_the_grant_the_server_issued() {
    let gone = SecretKey::generate().public();
    let grants = Arc::new(Grants::new(Removed::new(gone, true)));
    let stranger = SecretKey::generate().public();
    let nobody = SecretKey::generate().public();
    let file = Hash::new(b"brokered");
    let _guard = grants.grant(stranger, HashAndFormat::raw(file));
    assert!(grants.connect(1, Some(stranger)).is_ok());
    assert!(grants.get(1, &GetRequest::blob(file)).is_ok());
    // With no grant, it is refused as before.
    assert!(grants.connect(2, Some(nobody)).is_err());
}

#[test]
fn a_synced_peer_gets_only_the_project_blobs_and_only_raw() {
    let member = SecretKey::generate().public();
    let stranger = SecretKey::generate().public();
    let file = Hash::new(b"synced");
    let grants = Arc::new(Grants::new(Arc::new(Member(member, file))));
    assert!(grants.connect(1, Some(stranger)).is_err());
    assert!(grants.connect(2, Some(member)).is_ok());
    assert!(grants.get(2, &GetRequest::blob(file)).is_ok());
    assert!(grants.get(2, &GetRequest::all(file)).is_err());
    assert!(
        grants
            .get(2, &GetRequest::blob(Hash::new(b"other")))
            .is_err()
    );
}

#[tokio::test]
async fn a_fetch_counts_the_bytes_sent_of_every_blob() {
    use beep_proto::watched;
    use iroh_blobs::provider::events::{TransferProgress, TransferStarted};

    let (tx, rx) = irpc::channel::mpsc::channel(8);
    let (sent, watched) = watch::channel(0);
    let counting = tokio::spawn(count(rx, Some(sent)));
    let started = |index, size| TransferStarted {
        index,
        hash: Hash::new(b"blob"),
        size,
    };

    tx.send(started(0, 64).into()).await.unwrap();
    tx.send(TransferProgress { end_offset: 64 }.into())
        .await
        .unwrap();
    tx.send(started(1, 1000).into()).await.unwrap();
    tx.send(TransferProgress { end_offset: 400 }.into())
        .await
        .unwrap();
    drop(tx);
    counting.await.unwrap();

    assert_eq!(watched::read(&watched, |bytes| *bytes), 464);
}
