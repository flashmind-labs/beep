//! Skill libraries: Git repositories of agent skills, which a desktop names in
//! `skill-library/<name>` with the commit every machine checks out and where its skills go.
//! Each node keeps a clone of every library at its revision (`store`), installs the picked
//! skills into the agents' own skills folders and into project checkouts, and uninstalls what
//! is no longer picked (`install`). It also serves `skill-library/1` (`follow`): inspecting a
//! repository, finding libraries already in the skills folders (`find`), and what it could not
//! install.

mod find;
mod follow;
mod install;
mod store;

pub(crate) use follow::{follow, serve};

use beep_mesh::Mesh;
use beep_proto::{node::skill_library::Problem, watched};
use iroh::EndpointId;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};
use tokio::sync::watch;

/// What the installer shares with `skill-library/1` and with skill sync, which leaves the
/// skills it installed globally out. Every clone shares one.
#[derive(Clone)]
pub(crate) struct Libraries {
    shared: Arc<Shared>,
}

struct Shared {
    /// The node's own folder, set when the installer starts.
    dir: OnceLock<PathBuf>,
    /// What the last pass could not fetch or install.
    problems: watch::Sender<Vec<Problem>>,
    /// The names of the skills installed in the agents' own skills folders; none before the
    /// installer read its record.
    global: watch::Sender<Option<BTreeSet<String>>>,
}

impl Default for Libraries {
    fn default() -> Self {
        Self {
            shared: Arc::new(Shared {
                dir: OnceLock::new(),
                problems: watch::Sender::new(Vec::new()),
                global: watch::Sender::new(None),
            }),
        }
    }
}

/// The other members online now.
pub(crate) fn online(mesh: &Mesh) -> BTreeSet<EndpointId> {
    let me = mesh.me();
    mesh.code_hosts()
        .into_iter()
        .filter(|status| status.online && status.device.id != me)
        .map(|status| status.device.id)
        .collect()
}

impl Libraries {
    /// The skills a library installed in the agents' own skills folders, by name. Before the
    /// installer read its record, the record in the node's folder `node_dir` is read here.
    /// Blocks.
    pub(crate) fn global_names(&self, node_dir: &Path) -> BTreeSet<String> {
        if let Some(names) = watched::read_sent(&self.shared.global, Clone::clone) {
            return names;
        }

        match install::Record::load(node_dir) {
            Ok(record) => record.global_names(),
            Err(e) => {
                log::debug!("skill libraries: the record not read: {e:#}");
                BTreeSet::new()
            }
        }
    }
}
