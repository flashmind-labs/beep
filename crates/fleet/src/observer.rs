//! A phone's place in the mesh: an observer that controls code hosts and hosts nothing. It
//! answers the mesh and join protocols only, so no session, terminal or sync runs on it.

use crate::error::{FleetError, Result};
use beep_mesh::{DeviceKind, JOIN_ALPN, JoinTicket, Key, Mesh, PEER_ALPN};
use iroh::{Endpoint, protocol::Router};

/// The ticket in `text`, which must be a desktop's phone pairing ticket.
pub fn phone_ticket(text: &str) -> Result<JoinTicket> {
    let ticket = JoinTicket::parse(text).map_err(|error| FleetError::refused(error.to_string()))?;
    if ticket.kind != DeviceKind::Desktop {
        return Err(FleetError::refused(
            "Create a Phone pairing ticket on desktop to give this phone project controls.",
        ));
    }
    Ok(ticket)
}

/// Marks this device as an observer (once; the mark replicates) and serves the mesh and join
/// protocols on `endpoint`. Hold the router for as long as `mesh` is in use.
pub async fn observe(mesh: &Mesh, endpoint: &Endpoint) -> Result<Router> {
    if mesh.is_member(mesh.me()) && !mesh.is_observer(mesh.me()) {
        mesh.set(Key::Observer(mesh.me().to_string()), Some(Vec::new()))
            .await?;
    }
    let (peer, join) = mesh.protocols();
    Ok(Router::builder(endpoint.clone())
        .accept(PEER_ALPN, peer)
        .accept(JOIN_ALPN, join)
        .spawn())
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::{SecretKey, endpoint::presets};
    use iroh_tickets::Ticket;
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::time::timeout;

    async fn bind() -> Endpoint {
        Endpoint::builder(presets::Minimal)
            .secret_key(SecretKey::generate())
            .transport_config(beep_mesh::transport_config())
            .bind()
            .await
            .expect("the endpoint binds")
    }

    #[tokio::test]
    async fn a_paired_phone_is_an_observer_the_desktop_lists_no_machine_for() {
        let desk_dir = TempDir::new().expect("a temp dir");
        let desk_endpoint = bind().await;
        let desktop = Mesh::create(desk_dir.path(), desk_endpoint.clone(), "desktop")
            .await
            .expect("the mesh is made");
        let _desktop_router = serve(&desktop, &desk_endpoint);
        let ticket = desktop
            .invite_observer(Duration::from_secs(3600))
            .await
            .expect("a ticket");
        let ticket = phone_ticket(&ticket.encode_string()).expect("a phone ticket");

        let phone_dir = TempDir::new().expect("a temp dir");
        let phone_endpoint = bind().await;
        let phone = Mesh::join(phone_dir.path(), phone_endpoint.clone(), &ticket, "phone")
            .await
            .expect("the phone joins");
        let _phone_router = observe(&phone, &phone_endpoint).await.expect("observing");

        assert!(phone.is_observer(phone.me()));
        let phone_id = phone.me();
        // The mark replicates to the desktop, whose code hosts are its own machine only.
        timeout(Duration::from_secs(60), async {
            let mut updates = desktop.updates();
            loop {
                updates.borrow_and_update();
                if desktop.is_observer(phone_id) {
                    return;
                }
                updates.changed().await.expect("the mesh is alive");
            }
        })
        .await
        .expect("the desktop learns the phone observes");
        assert!(desktop.is_member(phone_id));
        let listed: Vec<_> = desktop.code_hosts().iter().map(|s| s.device.id).collect();
        assert_eq!(listed, vec![desk_endpoint.id()]);
        let seen: Vec<_> = phone.code_hosts().iter().map(|s| s.device.id).collect();
        assert_eq!(seen, vec![desk_endpoint.id()]);
    }

    fn serve(mesh: &Mesh, endpoint: &Endpoint) -> Router {
        let (peer, join) = mesh.protocols();
        Router::builder(endpoint.clone())
            .accept(PEER_ALPN, peer)
            .accept(JOIN_ALPN, join)
            .spawn()
    }
}
