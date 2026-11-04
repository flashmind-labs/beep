//! This Mac's side of the mesh: opening or making it, and serving the
//! endpoint's protocols for it.

use beep_mesh::{JOIN_ALPN, Mesh, MeshError, PEER_ALPN};
use beep_node::{
    files::{Folders, transfer::Transfers},
    link::peers::Peers,
};
use beep_proto::{client::PushHandler, wire::PUSH_ALPN};
use iroh::{Endpoint, protocol::Router};
use std::{env, path::Path, sync::Arc};

/// The mesh in `dir`, or a new one with this Mac, a desktop, as its first
/// device. No account or server comes into it.
pub(super) async fn open_or_create(
    dir: &Path,
    endpoint: &Endpoint,
    name: &str,
) -> Result<Mesh, MeshError> {
    match Mesh::open(dir, endpoint.clone()).await? {
        Some(mesh) => Ok(mesh),
        None => Mesh::create(dir, endpoint.clone(), name).await,
    }
}

/// What answers on the endpoint while the app runs.
pub(super) struct Serving {
    router: Router,
    transfers: Option<Transfers>,
}

impl Serving {
    /// Starts the node's transfer and sync services, as a node does, and
    /// serves the mesh, joins, those services and, with a server, its
    /// notices on `endpoint`.
    pub(super) async fn start(
        endpoint: &Endpoint,
        dir: &Path,
        peers: &Peers,
        push: Option<PushHandler>,
    ) -> Self {
        let mut router = Router::builder(endpoint.clone())
            .accept(PEER_ALPN, peers.peer_handler())
            .accept(JOIN_ALPN, peers.join_handler());
        if let Some(push) = push {
            router = router.accept(PUSH_ALPN, push);
        }
        let mut started = None;
        // Sync follows the user's folders, wherever the projects are.
        let folders = Folders::new(
            &env::var_os("HOME")
                .map(Into::into)
                .into_iter()
                .collect::<Vec<_>>(),
        );
        if !folders.is_empty() {
            match Transfers::start_shared(endpoint, &dir.join("blobs"), folders.clone()).await {
                Ok((transfers, served)) => {
                    router = served.add_to(router);
                    peers.offer(Arc::new(transfers.syncs()));
                    peers.offer(Arc::new(transfers.syncs().controls()));
                    peers.offer(Arc::new(transfers.syncs().rescans()));
                    peers.offer(Arc::new(transfers.clone()));
                    started = Some(transfers);
                }
                Err(e) => log::warn!("not offering transfers: {e:#}"),
            }
            peers.offer(Arc::new(folders));
        }
        Self {
            router: router.spawn(),
            transfers: started,
        }
    }

    /// Stops the router, then the services, which save their state.
    pub(super) async fn stop(self) {
        if let Err(e) = self.router.shutdown().await {
            log::warn!("the router did not stop cleanly: {e:#}");
        }
        if let Some(transfers) = self.transfers
            && let Err(e) = transfers.shutdown().await
        {
            log::warn!("transfers did not stop cleanly: {e:#}");
        }
    }
}
