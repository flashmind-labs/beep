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

/// A node that pushes its sync status on a watch and answers no direct call.
struct PushingPeers {
    mesh: beep_mesh::Mesh,
    pushed: tokio::sync::watch::Sender<beep_mesh::Followed>,
}

struct Pushed(tokio::sync::watch::Sender<beep_mesh::Followed>);

impl beep_mesh::SnapshotReader for Pushed {
    fn snapshot(&self, _read: Read) -> beep_mesh::SnapshotFuture<'_> {
        Box::pin(async { None })
    }

    fn follow(&self, _read: Read) -> Option<tokio::sync::watch::Receiver<beep_mesh::Followed>> {
        Some(self.0.subscribe())
    }
}

#[async_trait::async_trait]
impl crate::Peers for PushingPeers {
    async fn mesh(&self) -> crate::Result<beep_mesh::Mesh> {
        Ok(self.mesh.clone())
    }

    fn me(&self) -> EndpointId {
        self.mesh.me()
    }

    async fn client(&self, _machine: EndpointId) -> crate::Result<beep_mesh::PeerClient> {
        let unasked: beep_mesh::SessionFn =
            std::sync::Arc::new(|_, _| panic!("the status is pushed, not asked"));
        Ok(beep_mesh::PeerClient::local(unasked)
            .with_snapshot_reader(std::sync::Arc::new(Pushed(self.pushed.clone()))))
    }
}

fn syncing(names: &[&str]) -> beep_mesh::Followed {
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
    Some(Ok(Value::Sync(Box::new(Reply::Status { projects }))))
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
    let (pushed, _) = tokio::sync::watch::channel(syncing(&["acme/widget"]));
    let peers = std::sync::Arc::new(PushingPeers {
        mesh: mesh.clone(),
        pushed: pushed.clone(),
    });
    let session =
        crate::session::Session::new(peers, crate::session::Options::default(), |_| {}).unwrap();
    let desk = session.desk().await.unwrap();

    let (stopped, ()) = tokio::join!(desk.stopped_syncing(mesh.me(), "acme/widget"), async {
        pushed.send_replace(syncing(&["acme/widget", "acme/other"]));
        pushed.send_replace(syncing(&["acme/other"]));
    });

    assert!(stopped);
    endpoint.close().await;
}
