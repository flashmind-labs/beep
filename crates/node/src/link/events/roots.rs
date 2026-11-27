//! The file roots a subscription's topics name, and the bindings kept for them.

use crate::link::Offers;
use anyhow::{Context, Result, bail};
use beep_proto::node::events::Topic;
use futures::future::join_all;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub(super) const MAX_FILE_ROOTS: usize = 512;
pub(super) const TOO_MANY_ROOTS: &str =
    "too many watched directories; subscribe to specific file roots";

pub(super) type FileBindings = BTreeMap<Topic, Vec<PathBuf>>;

pub(super) struct FileWatcher {
    pub(super) paths: crate::events::paths::Paths,
    pub(super) bindings: Arc<Mutex<FileBindings>>,
}

pub(super) fn broad_files(topics: &[Topic]) -> bool {
    topics
        .iter()
        .any(|topic| topic.name() == "*" || (topic.name() == "files" && topic.key().is_none()))
}

/// The roots `topics` name now, by topic.
pub(super) async fn resolve(offers: &Offers, topics: &[Topic]) -> Result<BTreeMap<Topic, PathBuf>> {
    let mut wanted = BTreeMap::new();

    if broad_files(topics) {
        for root in offers.agents.bridge.notification_roots().await? {
            let key = root.to_str().context("a notification root is not UTF-8")?;
            wanted.insert(Topic::keyed("files", key), root);
        }
    }

    // The roots are checked together: each check can wait on a worktree list.
    let projects: Vec<_> = project_files(offers)
        .into_iter()
        .filter(|(topic, _)| topics.iter().any(|wanted| wanted.covers(topic)))
        .collect();
    let reached = join_all(projects.iter().map(|(_, path)| {
        offers
            .agents
            .reach
            .reachable(path, crate::agents::reach::Access::Read)
    }))
    .await;

    for ((topic, _), reached) in projects.into_iter().zip(reached) {
        match reached {
            Ok(root) => {
                wanted.insert(topic, root);
            }
            Err(error) if topics.iter().any(|wanted| wanted == &topic) => return Err(error),
            Err(_) => {}
        }
    }

    let literal: Vec<_> = topics
        .iter()
        .filter(|topic| topic.name() == "files" && !topic.is_pattern())
        .filter_map(|topic| match topic.resource_segments() {
            // Local paths are opaque literal segments. Project aliases use their mesh names.
            [path] if std::path::Path::new(path).is_absolute() || path.starts_with("~/") => {
                Some((topic, path))
            }
            _ => None,
        })
        .collect();
    let reached = join_all(literal.iter().map(|(_, path)| {
        offers
            .agents
            .reach
            .reachable(path, crate::agents::reach::Access::Read)
    }))
    .await;

    for ((topic, _), root) in literal.into_iter().zip(reached) {
        wanted.insert(topic.clone(), root?);
    }

    if wanted.len() > MAX_FILE_ROOTS {
        bail!(TOO_MANY_ROOTS);
    }

    Ok(wanted)
}

/// Whether a root's bindings include its Git metadata after the root itself.
pub(super) fn has_git(dependencies: &[PathBuf]) -> bool {
    dependencies.len() > 1
}

/// Resolve public project aliases through this node's signed mesh placement records.
fn project_files(offers: &Offers) -> BTreeMap<Topic, String> {
    use beep_mesh::Key;
    use beep_proto::node::mesh_state::{StateValue, SyncSpec};
    let Some(mesh) = offers.mesh.get() else {
        return BTreeMap::new();
    };
    let mut projects = BTreeMap::new();

    for (key, bytes) in mesh.scan("sync/") {
        if let Key::Sync(name) = key
            && let Ok(spec) = SyncSpec::from_bytes(&bytes)
            && let Some(folder) = spec.folders.get(&mesh.me())
        {
            let topic = Topic::scoped("files", name.split('/'));
            if topic.valid() {
                projects.insert(topic, folder.clone());
            }
        }
    }

    for (name, _, place) in crate::link::from_mesh::placed_projects(&mesh, mesh.me()) {
        let topic = Topic::scoped("files", name.split('/'));
        if topic.valid() {
            projects.insert(topic, place.folder);
        }
    }

    projects
}
