use super::*;
use beep_proto::{SecretText, node::EnvSecret};

#[tokio::test]
async fn tool_check_waits_for_the_first_result_and_returns_it() {
    let _env = spawn_env::TEST_LOCK.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let provision = Provision::new(dir.path());
    let link = beep_proto::node::local_link();
    let (mut tx, mut rx) = link.caller;
    let (node_tx, node_rx) = link.node;
    let server = provision.clone();
    let serving = tokio::spawn(async move { server.serve_tool_checks(node_tx, node_rx).await });
    tx.send(&Check {}).await.unwrap();
    provision.updates.send_replace(Some(vec![Available {
        tool: Tool::Flsh,
        version: "0.1.0".into(),
        latest: "0.2.0".into(),
    }]));
    let updates = rx.recv::<Vec<Available>>().await.unwrap().unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].tool, Tool::Flsh);
    assert_eq!(updates[0].latest, "0.2.0");
    serving.await.unwrap().unwrap();
}

#[tokio::test]
async fn forgetting_removes_the_tokens_from_disk() {
    let _env = spawn_env::TEST_LOCK.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let teacli = dir.path().join("bin/teacli");
    fs::create_dir_all(teacli.parent().unwrap()).unwrap();
    fs::write(&teacli, "#!/bin/sh\n").unwrap();
    crate::fs::fsat::set_mode(&teacli, 0o755).unwrap();
    let provision = Provision::new(dir.path());
    provision
        .apply(MachineCredentials {
            env: vec![EnvSecret {
                name: "BEEP_TEST_FORGET".into(),
                value: SecretText::new("key"),
            }],
            forgejo: vec![ForgejoLogin {
                url: "https://git.example.com".into(),
                token: SecretText::new("token"),
            }],
            agents: Vec::new(),
        })
        .await;
    let config = teacli::config_path(dir.path());
    assert!(config.exists());
    provision.forget().await;
    assert!(!config.exists());
    assert!(!teacli::shims_dir(dir.path()).join("teacli").exists());
    let mut command = tokio::process::Command::new("sh");
    command.args(["-c", "printf %s \"$BEEP_TEST_FORGET\""]);
    spawn_env::apply(&mut command, spawn_env::Scope::Shell);
    let out = command.output().await.unwrap();
    assert!(out.stdout.is_empty());
}

// A desktop's "Install Node" must not wait for the pinned tools' downloads,
// while two installs of one tool still take turns.
#[tokio::test]
async fn installs_of_different_tools_overlap_and_of_one_tool_take_turns() {
    use futures::FutureExt;

    let installs = Installs::default();
    let gh = installs.lock(Tool::Gh).await;

    assert!(installs.lock(Tool::Teacli).now_or_never().is_some());
    assert!(installs.lock(Need::Node).now_or_never().is_some());

    let mut second = Box::pin(installs.lock(Tool::Gh));
    assert!(second.as_mut().now_or_never().is_none());
    drop(gh);
    assert!(second.now_or_never().is_some());
}

#[test]
fn a_check_is_published_for_the_snapshot_only_when_it_differs() {
    let dir = tempfile::tempdir().unwrap();
    let provision = Provision::new(dir.path());
    let flsh = Available {
        tool: Tool::Flsh,
        version: "0.1.0".into(),
        latest: "0.2.0".into(),
    };

    assert_eq!(provision.updatable(), None);
    assert!(provision.publish(Vec::new()));
    assert_eq!(provision.updatable(), Some(Vec::new()));
    assert!(!provision.publish(Vec::new()));
    assert!(provision.publish(vec![flsh.clone()]));
    assert_eq!(provision.updatable(), Some(vec![flsh]));
}
