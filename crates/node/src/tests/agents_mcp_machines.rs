use super::*;
use beep_proto::node::tools::PROTOCOL;
use futures::FutureExt;
use iroh::{Endpoint, SecretKey, endpoint::presets};

#[tokio::test]
async fn an_offline_machine_gets_one_setup_wait_across_all_retries() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal)
        .bind()
        .await
        .unwrap();
    let mesh = Mesh::create(dir.path(), endpoint.clone(), "build-box")
        .await
        .unwrap();
    let peer = SecretKey::generate().public();

    tokio::time::pause();
    let call = handshake(&mesh, peer, PROTOCOL);
    tokio::pin!(call);
    assert!(call.as_mut().now_or_never().is_none());
    tokio::time::advance(Duration::from_secs(14)).await;
    assert!(call.as_mut().now_or_never().is_none());

    let error = timeout(Duration::from_secs(2), call)
        .await
        .expect("setup must finish after one connection wait")
        .err()
        .expect("the offline machine cannot accept a call");
    assert!(error.to_string().contains("the request was not sent"));
    tokio::time::resume();
    endpoint.close().await;
}

#[test]
fn a_job_id_names_its_machine() {
    let named = name_the_job(
        "Started it in the background. Job id: 7. Check it.",
        "box-b",
    );
    assert_eq!(
        named,
        "Started it in the background. Job id: task:7@box-b. Check it."
    );
    let job = "task:7@box-b".parse::<Job>().expect("a job id");
    assert_eq!((job.id, job.machine.as_str()), (7, "box-b"));
    assert!("7".parse::<Job>().ok().is_none());
}

#[test]
fn a_machine_is_named_in_one_word() {
    assert_eq!(command_name(" Mac Studio "), "mac-studio");
    assert_eq!(command_name("Alex’s Mac Studio"), "alexs-mac-studio");
    assert_eq!(command_name("Alex's  MacBook (2)"), "alexs-macbook-2");
}
