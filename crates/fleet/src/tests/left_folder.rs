use super::*;

#[test]
fn only_a_folder_that_holds_just_the_project_is_deleted() {
    let none: Vec<String> = Vec::new();
    assert_eq!(why_kept("/srv/widget", &["/home/user"], &none), None);
    assert_eq!(why_kept(r"\\?\C:\Users\me\widget", &[], &none), None);
    assert!(why_kept("/srv", &[], &none).is_some());
    assert!(why_kept(r"C:\", &[], &none).is_some());
    assert!(why_kept("/home/user/", &["/home/user"], &none).is_some());
    assert!(why_kept(r"C:\Users\Me", &[r"c:\users\me"], &none).is_some());
    let others = vec!["/srv/widget/tools".to_owned()];
    assert!(why_kept("/srv/widget", &[], &others).is_some());
    let others = vec!["/srv".to_owned(), "/srv/widget-old".to_owned()];
    assert!(why_kept("/srv/widget", &[], &others).is_some());
    let others = vec!["/srv/widget-old".to_owned()];
    assert_eq!(why_kept("/srv/widget", &[], &others), None);
}

fn syncing(names: &[&str]) -> Value {
    let projects = names
        .iter()
        .map(|name| beep_proto::node::sync::ProjectStatus {
            name: (*name).into(),
            folder: "/srv/widget".into(),
            state: beep_proto::node::sync::State::Idle,
            paused: false,
            git_only: false,
            files: 0,
            bytes: 0,
            pending: 0,
            waiting: Vec::new(),
            progress: None,
            conflicts: Vec::new(),
            rescan_secs: None,
            hint: None,
            error: None,
        })
        .collect();
    Value::Sync(Box::new(Reply::Status { projects }))
}

#[tokio::test]
async fn a_project_is_stopped_when_the_node_pushes_a_status_without_it() {
    let dir = tempfile::TempDir::new().unwrap();
    let endpoint = iroh::Endpoint::builder(iroh::endpoint::presets::Minimal)
        .transport_config(beep_mesh::transport_config())
        .bind()
        .await
        .unwrap();
    let mesh = beep_mesh::Mesh::create(dir.path(), endpoint.clone(), "desktop")
        .await
        .unwrap();
    // A node that pushes its sync status and answers no direct call.
    let unasked: beep_mesh::SessionFn =
        std::sync::Arc::new(|_, _| panic!("the status is pushed, not asked"));
    let machines = std::sync::Arc::new(crate::Machines::new(mesh.me(), Some(unasked)));
    machines.set_opened(crate::Opened::Open(mesh.clone()));
    let pushed = machines.snapshots().clone();
    pushed.open_stream(mesh.me(), Read::SyncList.topic().unwrap());
    pushed.push(mesh.me(), Read::SyncList, syncing(&["acme/widget"]));
    let session =
        crate::session::Session::new(machines, crate::session::Options::default()).unwrap();
    let desk = session.desk().await.unwrap();

    let (stopped, ()) = tokio::join!(desk.stopped_syncing(mesh.me(), "acme/widget"), async {
        pushed.push(
            mesh.me(),
            Read::SyncList,
            syncing(&["acme/widget", "acme/other"]),
        );
        pushed.push(mesh.me(), Read::SyncList, syncing(&["acme/other"]));
    });

    assert!(stopped);
    endpoint.close().await;
}
