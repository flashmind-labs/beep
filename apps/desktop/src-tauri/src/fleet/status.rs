#[cfg(test)]
mod tests {
    use crate::fleet::tests::{join, online, start, until, LIMIT};
    use beep_mesh::DeviceKind;
    use tokio::time::timeout;

    use super::Status;

    /// Waits until `ok` holds for what the machines have reported.
    async fn settled(status: &Status, ok: impl Fn(&Status) -> bool) {
        let mut changes = status.changes();
        timeout(LIMIT, async {
            loop {
                changes.borrow_and_update();
                if ok(status) {
                    return;
                }
                changes.changed().await.expect("the status is alive");
            }
        })
        .await
        .expect("the status settled in time");
    }

    #[tokio::test]
    async fn a_machine_list_shows_who_is_on_and_off() {
        let mac = start("Studio").await;
        let host = mac.fleet.mesh().expect("the mesh is open");
        let (mesh, router, endpoint) = join(&mac.fleet, DeviceKind::Node, "Server").await;
        let (this, other) = (mac.endpoint.id().to_string(), endpoint.id().to_string());
        let status = mac.fleet.status();
        // This Mac's node says what it is; the other machine is on.
        settled(status, |status| {
            let machines = status.machines();
            let on = |id: &str| machines.iter().any(|m| m.id == id && m.online);
            on(&this) && on(&other) && status.details().len() == 2
        })
        .await;
        let machines = status.machines();
        let studio = machines.iter().find(|m| m.id == this).expect("listed");
        assert_eq!(studio.name, "Studio");
        // Switched off: its endpoint closes, and with it the link.
        router.shutdown().await.expect("the router stops");
        drop(mesh);
        endpoint.close().await;
        until(&host, |mesh| !online(mesh, endpoint.id())).await;
        settled(status, |status| {
            status
                .machines()
                .iter()
                .any(|m| m.id == other && !m.online)
        })
        .await;
        let machines = status.machines();
        assert!(machines.iter().any(|m| m.id == this && m.online));
        assert!(machines.iter().any(|m| m.id == other && !m.online));
    }

    #[tokio::test]
    async fn removing_a_machine_cuts_it_off_from_every_other() {
        let mac = start("Studio").await;
        let host = mac.fleet.mesh().expect("the mesh is open");
        let (first, _first_router, first_end) = join(&mac.fleet, DeviceKind::Node, "Box").await;
        let (second, _second_router, second_end) = join(&mac.fleet, DeviceKind::Node, "Rig").await;
        let (gone, kept) = (first_end.id(), second_end.id());
        until(&host, |mesh| online(mesh, gone) && online(mesh, kept)).await;
        until(&second, |mesh| online(mesh, gone)).await;
        mac.fleet.remove(gone).await.expect("a desktop removes it");
        // The others drop it from their lists and lose its link.
        until(&second, |mesh| !mesh.is_member(gone)).await;
        assert!(!host.is_member(gone));
        assert!(!second.devices().iter().any(|d| d.device.id == gone));
        until(&first, |mesh| !online(mesh, mac.fleet.id())).await;
        let status = mac.fleet.status();
        settled(status, |status| {
            status.machines().iter().all(|m| m.id != gone.to_string())
        })
        .await;
        assert!(
            mac.fleet.client(gone).await.is_err(),
            "a removed machine is not reachable"
        );
    }
}
