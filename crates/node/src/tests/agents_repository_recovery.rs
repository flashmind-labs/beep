use super::*;
use crate::agents::{AgentKind, Begin};

#[tokio::test]
async fn recovery_blocks_new_sessions_and_open_sessions_block_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let kind = AgentKind {
        name: "test".into(),
        program: "sh".into(),
        args: vec!["-c".into(), "while read -r line; do :; done".into()],
        env: Default::default(),
        compact: None,
        compact_models: Vec::new(),
        default_model: None,
        bridge: None,
    };
    let agents = Arc::new(Agents::new(vec![kind.clone()]));
    let hold = agents.reserve_repository(root).await.unwrap();
    let blocked = agents.start(&kind, root.to_str(), Begin::default());
    assert!(
        blocked
            .await.err()
            .unwrap()
            .to_string()
            .contains("preserving local edits")
    );
    let alias = dir.path().join("alias");
    std::os::unix::fs::symlink(root, &alias).unwrap();
    let blocked = agents.start(&kind, alias.to_str(), Begin::default());
    assert!(
        blocked
            .await.err()
            .unwrap()
            .to_string()
            .contains("preserving local edits")
    );
    drop(hold);
    let session = agents
        .start(&kind, alias.to_str(), Begin::default())
        .await.unwrap();
    let blocked = agents.reserve_repository(root).await;
    assert!(
        blocked
            .err()
            .unwrap()
            .to_string()
            .contains("close running sessions")
    );
    session.close().await;
    agents.reserve_repository(root).await.unwrap();
}
