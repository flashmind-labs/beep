//! Keeps the agents' adapters installed: at start and every hour after, the
//! node detects its agents again, installs the adapters missing, one at a
//! time, and offers what it can start now.

use super::{
    adapters::{install, installs::Install, layout},
    catalog::Catalog,
    kinds::{self, AgentKind, Detection},
};
use crate::{known::Known, status::Agent};
use beep_proto::node::AdapterInstall;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{sync::watch, task::spawn_blocking};
use tokio_util::task::AbortOnDropHandle;

/// How long the node waits between two detections, and so before it tries
/// a failed install again.
const RECHECK: Duration = Duration::from_secs(60 * 60);

impl Catalog {
    /// Installs the adapters the agents found here lack into the node's
    /// folder `node_dir`, in the background, now and on every detection
    /// after; `extra` are the agents from `node.toml`. Dropping the handle
    /// stops the work.
    pub fn install_adapters(
        self: &Arc<Self>,
        node_dir: PathBuf,
        extra: Vec<AgentKind>,
    ) -> AbortOnDropHandle<()> {
        let catalog = self.clone();
        AbortOnDropHandle::new(tokio::spawn(async move {
            loop {
                catalog.keep_adapters(&node_dir, &extra).await;
                tokio::select! {
                    () = tokio::time::sleep(RECHECK) => {}
                    () = catalog.adapters.again.notified() => {}
                    () = crate::machine::wsl::agents_changed() => {}
                }
            }
        }))
    }

    /// Records what `detection` found and sets the agents it can start;
    /// the known agents still lacking an adapter come back.
    fn apply(&self, detection: Detection) -> Vec<&'static Known> {
        self.adapters.saw(&detection);
        self.set_kinds(detection.kinds);
        detection.lacking
    }

    /// One detection, and the installs it calls for, one at a time.
    async fn keep_adapters(&self, node_dir: &Path, extra: &[AgentKind]) {
        let Some(detection) = detect(node_dir, extra).await else {
            return;
        };
        let lacking = self.apply(detection);

        if lacking.is_empty() {
            return;
        }

        let root = layout::root(node_dir);

        for known in lacking {
            log::info!("{} found, installing its adapter", known.cli);
            self.adapters.set(known.name, Install::Running);

            match install::adapter(&root, known).await {
                Ok(installed) => {
                    log::info!(
                        "{} adapter installed at {}",
                        known.name,
                        installed.script.display()
                    );
                    self.adapters.set(known.name, Install::Done);

                    if let Some(after) = detect(node_dir, extra).await {
                        self.apply(after);
                    }
                }
                Err(e) => {
                    log::warn!(
                        "{} adapter not installed, trying again in an hour: {e:#}",
                        known.name
                    );
                    self.adapters
                        .set(known.name, Install::Failed(format!("{e:#}")));
                }
            }
        }
    }

    /// Detects and installs the missing adapters now, as Try again asks:
    /// the failed ones read as installing at once.
    pub(crate) fn install_adapters_now(&self) {
        self.adapters.states.send_if_modified(|installs| {
            let mut any = false;

            for install in installs.values_mut() {
                if matches!(install, Install::Failed(_)) {
                    *install = Install::Running;
                    any = true;
                }
            }

            any
        });
        self.adapters.again.notify_one();
    }

    /// The agents this node offers and how their adapters stand, for
    /// `beep-node status`.
    pub(crate) fn agent_report(&self) -> Vec<Agent> {
        self.adapters.report(&self.kinds())
    }

    /// The adapters on their way or failed, for `EnvReport::adapters`.
    pub(crate) fn adapter_installs(&self) -> Vec<AdapterInstall> {
        self.adapters.pending()
    }

    /// Changes each time an adapter install starts, ends or is forgotten.
    pub(crate) fn adapters_changed(&self) -> watch::Receiver<BTreeMap<&'static str, Install>> {
        self.adapters.states.subscribe()
    }
}

/// `kinds::detection` off the async workers: it looks on disk.
async fn detect(node_dir: &Path, extra: &[AgentKind]) -> Option<Detection> {
    let node_dir = node_dir.to_owned();
    let extra = extra.to_vec();

    match spawn_blocking(move || kinds::detection(&extra, Some(&node_dir))).await {
        Ok(detection) => Some(detection),
        Err(e) => {
            log::warn!("detecting the coding agents failed: {e}");
            None
        }
    }
}

#[cfg(test)]
#[path = "../tests/agents_adapters_worker.rs"]
mod tests;
