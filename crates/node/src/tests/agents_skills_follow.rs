use super::*;
use beep_proto::node::mesh_state::{SkillSync, StateValue};
use iroh::{Endpoint, SecretKey, endpoint::presets};

/// A machine that did not answer is read again alone: the ones that did are
/// not asked and the pass does not start over.
#[tokio::test]
async fn only_the_machines_that_did_not_answer_are_read_again() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal).bind().await.unwrap();
    let mesh = Mesh::create(&dir.path().join("mesh"), endpoint, "build-box")
        .await
        .unwrap();
    mesh.set(Key::SkillSync, Some(SkillSync { enabled: true }.to_bytes().unwrap()))
        .await
        .unwrap();
    let mut pass = Passes {
        mesh: mesh.clone(),
        home: dir.path().join("home"),
        dir: dir.path().join("node"),
        seen: Seen::default(),
        hashes: Hashes::default(),
        asked: false,
        unread: BTreeSet::new(),
        unheard: false,
        compared: 0,
        lists: HashMap::new(),
        sent: HashMap::new(),
        told: BTreeMap::new(),
        said: None,
        skills: Skills::default(),
    };
    let (silent, answered) = (SecretKey::generate().public(), SecretKey::generate().public());
    let online = BTreeSet::from([silent, answered]);

    pass.ask(&online, true).await;
    assert_eq!(pass.unread, online);

    // One of them answered since.
    pass.unread.remove(&answered);
    pass.ask(&online, false).await;

    assert_eq!(pass.unread, BTreeSet::from([silent]));
}
