use super::*;
use crate::Peers;
use async_trait::async_trait;
use beep_mesh::{Mesh, PeerClient, transport_config};
use beep_proto::node::mesh_state::StateValue;
use iroh::{Endpoint, endpoint::presets};
use std::sync::Arc;
use tempfile::TempDir;

struct TestPeers(Mesh);

#[async_trait]
impl Peers for TestPeers {
    async fn mesh(&self) -> Result<Mesh> {
        Ok(self.0.clone())
    }

    fn me(&self) -> EndpointId {
        self.0.me()
    }

    async fn client(&self, _machine: EndpointId) -> Result<PeerClient> {
        Err(FleetError::Offline("offline".into()))
    }
}

#[tokio::test]
async fn master_pause_preserves_preferences_and_resumes_new_projects() {
    let dir = TempDir::new().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal)
        .transport_config(transport_config())
        .bind()
        .await
        .unwrap();
    let mesh = Mesh::create(dir.path(), endpoint.clone(), "desktop")
        .await
        .unwrap();
    let desk = Desk::open(Arc::new(TestPeers(mesh.clone()))).await.unwrap();
    for (name, paused) in [
        ("acme/one", false),
        ("acme/two", false),
        ("acme/paused", true),
    ] {
        mesh.set(
            Key::Sync(name.into()),
            Some(
                SyncSpec {
                    paused,
                    ..SyncSpec::default()
                }
                .to_bytes()
                .unwrap(),
            ),
        )
        .await
        .unwrap();
    }
    assert!(desk.project_sync_enabled());
    desk.set_project_sync_enabled(false).await.unwrap();
    assert!(!desk.project_sync_enabled());
    assert!(
        scan::<SyncSpec>(&mesh, "sync/")
            .iter()
            .all(|(_, spec)| spec.paused)
    );
    // Retrying disable must not treat the master pause as each project's preference.
    desk.set_project_sync_enabled(false).await.unwrap();
    mesh.set(
        Key::Sync("acme/new".into()),
        Some(
            SyncSpec {
                paused: true,
                ..SyncSpec::default()
            }
            .to_bytes()
            .unwrap(),
        ),
    )
    .await
    .unwrap();
    desk.pause_sync("acme/one", true).await.unwrap();
    desk.set_project_sync_enabled(true).await.unwrap();
    assert!(desk.project_sync_enabled());
    assert!(
        read::<SyncSpec>(&mesh, &Key::Sync("acme/one".into()))
            .unwrap()
            .paused
    );
    assert!(
        read::<SyncSpec>(&mesh, &Key::Sync("acme/paused".into()))
            .unwrap()
            .paused
    );
    assert!(
        !read::<SyncSpec>(&mesh, &Key::Sync("acme/two".into()))
            .unwrap()
            .paused
    );
    assert!(
        !read::<SyncSpec>(&mesh, &Key::Sync("acme/new".into()))
            .unwrap()
            .paused
    );
    endpoint.close().await;
}
