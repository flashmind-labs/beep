use super::*;
use iroh::SecretKey;
use iroh_blobs::Hash;

fn granted(answer: Answer) -> bool {
    matches!(answer, Answer::Granted(_))
}

#[test]
fn only_the_granted_peer_gets_the_granted_root() {
    let grants = Arc::new(Grants::new());
    let peer = SecretKey::generate().public();
    let stranger = SecretKey::generate().public();
    let file = Hash::new(b"file");
    let folder = Hash::new(b"folder");
    let guard = grants.grant(peer, HashAndFormat::raw(file));
    let folder_guard = grants.grant(peer, HashAndFormat::hash_seq(folder));

    assert!(grants.connect(1, Some(stranger), false).is_err());
    assert!(grants.connect(2, None, false).is_err());
    assert!(grants.connect(3, Some(peer), false).is_ok());
    assert!(granted(grants.get(3, &GetRequest::blob(file))));
    assert!(granted(grants.get(3, &GetRequest::all(folder))));
    // A file's hash read as a folder would reach other blobs.
    assert!(!granted(grants.get(3, &GetRequest::all(file))));
    assert!(!granted(grants.get(3, &GetRequest::blob(Hash::new(b"other")))));
    assert!(!granted(grants.get(1, &GetRequest::blob(file))));

    drop(guard);
    assert!(!granted(grants.get(3, &GetRequest::blob(file))));
    drop(folder_guard);
    assert!(grants.connect(4, Some(peer), false).is_err());
    grants.closed(3);
    assert!(!granted(grants.get(3, &GetRequest::all(folder))));
}

#[test]
fn a_synced_peer_gets_only_the_project_blobs_and_only_raw() {
    let member = SecretKey::generate().public();
    let stranger = SecretKey::generate().public();
    let file = Hash::new(b"synced");
    let grants = Arc::new(Grants::new());
    assert!(grants.connect(1, Some(stranger), false).is_err());
    assert!(grants.connect(2, Some(member), true).is_ok());
    assert_eq!(grants.peer(2), Some(member));
    // A blob with no grant is left to sync, a folder read of it is refused.
    assert!(matches!(
        grants.get(2, &GetRequest::blob(file)),
        Answer::NoGrant(peer) if peer == member
    ));
    assert!(matches!(
        grants.get(2, &GetRequest::all(file)),
        Answer::Refused
    ));
}
