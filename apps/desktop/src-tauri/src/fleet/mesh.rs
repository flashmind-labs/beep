//! This Mac's side of the mesh: opening or making it, and serving the
//! endpoint's protocols for it.

use beep_mesh::{JOIN_ALPN, Mesh, MeshError, PEER_ALPN};
use beep_node::{
    files::{
        Folders,
        transfer::{Late, Transfers},
    },
    link::peers::Peers,
};
use beep_proto::{
    client::PushHandler,
    node::{files, sync, transfer},
    wire::PUSH_ALPN,
};
use iroh::{Endpoint, protocol::Router};
use std::{
    env, mem,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

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

/// Protocols the node serves once its transfer and sync services are up. A
/// session that opens one before then waits for them.
pub(super) const LATER: [&str; 5] = [
    sync::PROTOCOL,
    sync::CONTROLS,
    sync::RESCAN,
    transfer::PROTOCOL,
    files::PROTOCOL,
];

/// What answers on the endpoint while the app runs.
pub(super) struct Serving {
    router: Router,
    late: Late,
    services: Mutex<Services>,
}

/// The sync and transfer services: not started yet, running, or stopped for
/// good, after which none starts.
enum Services {
    Waiting,
    Running(Transfers),
    Stopped,
}

/// Lets the sessions and connections waiting for the services go on, when
/// starting them ends any way at all, a panic included.
struct Settle<'a> {
    late: &'a Late,
    peers: &'a Peers,
}

impl Drop for Settle<'_> {
    fn drop(&mut self) {
        self.late.fail();
        self.peers.settle();
    }
}

impl Serving {
    /// Serves the mesh, joins and, with a server, its notices on `endpoint`
    /// at once. Connections for blobs and sync wait for `start_services`.
    pub(super) fn spawn(endpoint: &Endpoint, peers: &Peers, push: Option<PushHandler>) -> Self {
        let mut router = Router::builder(endpoint.clone())
            .accept(PEER_ALPN, peers.peer_handler())
            .accept(JOIN_ALPN, peers.join_handler());
        if let Some(push) = push {
            router = router.accept(PUSH_ALPN, push);
        }
        let late = Late::default();
        Self {
            router: late.add_to(router).spawn(),
            late,
            services: Mutex::new(Services::Waiting),
        }
    }

    /// Starts the node's transfer and sync services, as a node does. Those
    /// that cannot start are left out, and nothing waits for them.
    pub(super) async fn start_services(&self, endpoint: &Endpoint, dir: &Path, peers: &Peers) {
        let _settle = Settle {
            late: &self.late,
            peers,
        };
        // Sync follows the user's folders, wherever the projects are.
        let home: Vec<PathBuf> = env::var_os("HOME").map(Into::into).into_iter().collect();
        let folders = Folders::new(&home);
        if folders.is_empty() {
            return;
        }
        match Transfers::start_shared(endpoint, &dir.join("blobs"), folders.clone()).await {
            Ok((transfers, served)) => {
                let stopped = {
                    let mut services = self.services();
                    let stopped = matches!(*services, Services::Stopped);
                    if !stopped {
                        self.late.serve(served);
                        peers.offer(Arc::new(transfers.syncs()));
                        peers.offer(Arc::new(transfers.syncs().controls()));
                        peers.offer(Arc::new(transfers.syncs().rescans()));
                        peers.offer(Arc::new(transfers.clone()));
                        peers.serve_transfers(transfers.clone());
                        *services = Services::Running(transfers.clone());
                    }
                    stopped
                };
                if stopped {
                    // Stopped while starting: these never run.
                    if let Err(e) = transfers.shutdown().await {
                        log::warn!("transfers did not stop cleanly: {e:#}");
                    }
                    return;
                }
            }
            Err(e) => {
                log::warn!("not offering transfers: {e:#}");
                return;
            }
        }
        peers.offer(Arc::new(folders));
    }

    fn services(&self) -> MutexGuard<'_, Services> {
        self.services.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Stops the router, then the services, which save their state.
    pub(super) async fn stop(&self) {
        if let Err(e) = self.router.shutdown().await {
            log::warn!("the router did not stop cleanly: {e:#}");
        }
        let services = mem::replace(&mut *self.services(), Services::Stopped);
        let transfers = match services {
            Services::Running(transfers) => Some(transfers),
            Services::Waiting | Services::Stopped => None,
        };
        if let Some(transfers) = transfers
            && let Err(e) = transfers.shutdown().await
        {
            log::warn!("transfers did not stop cleanly: {e:#}");
        }
    }
}
