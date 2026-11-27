//! What a node knows of itself, as values: its key, name and folder. The
//! diagnostics a problem report reads are served from these and the `Node`,
//! so what asks for them needs no handle on the running node.

use super::read_open;
use crate::{
    link::{protocol::SessionProtocol, report::serve_report},
    status::{self, LocalStatus, Own},
    tools::Node,
};
use anyhow::{Result, bail};
use beep_mesh::{DeviceKind, Mesh, PeerClient, SessionFn};
use beep_proto::node::{LinkRx, LinkTx, Opened, report};
use beep_report::Target;
use std::{
    path::Path,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

impl Own {
    /// Whether `protocol` is one every machine of the mesh may read, shared
    /// or not.
    pub(crate) fn is_diagnostic(protocol: &SessionProtocol) -> bool {
        matches!(
            protocol,
            SessionProtocol::LogQuery
                | SessionProtocol::Metrics
                | SessionProtocol::MetricsQuery
                | SessionProtocol::Report
        )
    }

    /// Serves a diagnostic `protocol` stream.
    pub(crate) async fn serve_diagnostic(
        &self,
        protocol: SessionProtocol,
        mut tx: LinkTx,
        rx: LinkRx,
        node: Node,
        compress: bool,
    ) -> Result<()> {
        if compress {
            tx.send(&Opened::ReadyCompressed {}).await?;
            tx.enable_zstd();
        } else {
            tx.send(&Opened::Ready {}).await?;
        }
        let metrics = self.dir.join("metrics.log");

        match protocol {
            SessionProtocol::LogQuery => node.log.serve_query(tx, rx).await,
            SessionProtocol::MetricsQuery => {
                crate::link::metrics::serve_query(tx, rx, metrics).await
            }
            SessionProtocol::Report => match self.local_status(&node).await {
                Ok(status) => serve_report(tx, rx, status, node.log, metrics).await,
                Err(e) => {
                    let reason = format!("this machine's status could not be read: {e:#}");
                    tx.send(&report::Reply::Failed { reason }).await?;
                    tx.finish().await
                }
            },
            SessionProtocol::Metrics => crate::link::metrics::serve(tx, rx, metrics).await,
            other => bail!("{other} is not a diagnostic"),
        }
    }

    /// A session of this node for the diagnostics only, for a caller in this
    /// process.
    fn diagnostics(&self, node: Node) -> SessionFn {
        let own = self.clone();

        Arc::new(move |tx: LinkTx, mut rx: LinkRx| {
            let (own, node) = (own.clone(), node.clone());
            Box::pin(async move {
                let served = async {
                    let (protocol, compress) = read_open(&mut rx).await?;
                    own.serve_diagnostic(protocol, tx, rx, node, compress).await
                };

                if let Err(e) = served.await {
                    log::debug!("a local diagnostic ended: {e:#}");
                }
            })
        })
    }

    /// What `beep-node status` shows. Checks each project's folder on disk.
    pub(crate) async fn local_status(&self, node: &Node) -> Result<LocalStatus> {
        let mesh = node.mesh.get().filter(|mesh| mesh.is_member(mesh.me()));
        let devices = mesh
            .as_ref()
            .map(Mesh::devices)
            .unwrap_or_default()
            .into_iter()
            .map(|status| status::Peer {
                id: status.device.id,
                name: status.device.name,
                kind: status.device.kind,
                online: status.online,
                last_seen: status.last_seen.map(unix_secs),
            })
            .collect();
        let syncs = node.transfers.get().map(|transfers| transfers.syncs());
        let projects = match syncs {
            Some(syncs) => {
                let held = syncs.held();
                let statuses = syncs.status();
                // A folder on a disk that hangs must not hold a worker.
                tokio::task::spawn_blocking(move || {
                    statuses
                        .into_iter()
                        .map(|status| status::Project {
                            held: held.contains(&status.name),
                            missing: !Path::new(&status.folder).exists(),
                            status,
                        })
                        .collect()
                })
                .await?
            }
            None => Vec::new(),
        };

        Ok(LocalStatus {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            started: unix_secs(self.started),
            name: self.name.clone(),
            id: self.endpoint.id(),
            mesh: mesh.map(|mesh| mesh.id()),
            devices,
            projects,
            agents: node.agents.catalog.agent_report(),
        })
    }

    /// What a problem report asks: this node through its own diagnostics, the
    /// others over their live links. This node alone before it joined a mesh.
    pub(crate) fn report_targets(&self, node: &Node) -> Vec<Target> {
        let me = self.endpoint.id();
        let own = |name: String, kind: DeviceKind| Target {
            id: me,
            name,
            kind,
            client: Ok(PeerClient::local(self.diagnostics(node.clone()))),
        };

        match node.mesh.get().filter(|mesh| mesh.is_member(me)) {
            Some(mesh) => mesh
                .code_hosts()
                .into_iter()
                .map(|status| {
                    let device = status.device;

                    if device.id == me {
                        return own(device.name, device.kind);
                    }

                    Target {
                        id: device.id,
                        name: device.name,
                        kind: device.kind,
                        client: mesh
                            .peer(device.id)
                            .map(PeerClient::new)
                            .ok_or_else(|| "offline".to_owned()),
                    }
                })
                .collect(),
            None => vec![own(self.name.clone(), DeviceKind::Node)],
        }
    }

    /// The key this node signs with, as a problem report is signed.
    pub(crate) fn key(&self) -> iroh::SecretKey {
        self.endpoint.secret_key().clone()
    }

    /// Whether this device is a desktop in the current device list.
    pub(crate) fn is_desktop(&self, node: &Node) -> bool {
        let me = self.endpoint.id();

        self.devices(node)
            .iter()
            .any(|status| status.device.id == me && status.device.kind == DeviceKind::Desktop)
    }

    /// The mesh's devices, empty before the node joined.
    pub(crate) fn devices(&self, node: &Node) -> Vec<beep_mesh::DeviceStatus> {
        node.mesh
            .get()
            .map(|mesh| mesh.devices())
            .unwrap_or_default()
    }
}

/// Seconds since the Unix epoch; 0 for a time before it.
fn unix_secs(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}
