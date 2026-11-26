//! Deleting the folder a project leaves on a machine it was taken off, when the user asks: only
//! once the node there has stopped syncing it, so the delete never reaches the other machines,
//! and never a folder that holds more than that project.

use crate::{desk::Desk, records::projects};
use beep_projects::{model::plain_path, quote};
use beep_proto::node::{
    events::{Read, Value},
    sync::Reply,
};
use iroh::EndpointId;
use std::time::Duration;
use tokio::time::timeout;

/// How long the node there has to stop syncing the project.
const STOP_WAIT: Duration = Duration::from_secs(20);
/// How long the delete may take.
const DELETE_LIMIT: Duration = Duration::from_secs(120);
/// What the script prints before what it did.
const MARK: &str = "BEEP_RM:";

impl Desk {
    /// Deletes `folder`, which `project` had on `machine`, and says what became of it.
    pub(crate) async fn delete_left_folder(
        &self,
        project: &str,
        machine: EndpointId,
        folder: &str,
    ) -> String {
        let shown = plain_path(folder).into_owned();
        let name = self.name_of(machine);
        let kept = |why: &str| format!("{shown} on {name} stays: {why}");
        let Some(seen) = self.seen_one(machine).await else {
            return kept("the machine is not in the mesh.");
        };
        let others: Vec<String> = projects(&self.mesh)
            .into_iter()
            .filter(|other| other.name != project)
            .filter_map(|other| other.places.get(&machine).map(|p| p.folder.clone()))
            .collect();
        let env = seen.env.as_ref();
        let roots: Vec<&str> = env
            .map(|env| {
                std::iter::once(env.home.as_str())
                    .chain(env.syncthing.iter().map(|f| f.path.as_str()))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(why) = why_kept(&shown, &roots, &others) {
            return kept(&why);
        }

        let Ok(shell) = seen.machine.shell() else {
            return kept("the machine is offline.");
        };

        if !self.stopped_syncing(machine, project).await {
            return kept("its node did not say it stopped syncing it.");
        }

        let script = format!(
            "d={word}\n\
             if command -v cygpath >/dev/null 2>&1; then d=$(cygpath -u -- \"$d\"); fi\n\
             if [ ! -e \"$d\" ]; then echo '{MARK} missing'; exit 0; fi\n\
             rm -rf -- \"$d\" 2>&1 && echo '{MARK} deleted'\n",
            word = quote(&shown),
        );

        match shell.run(&script, DELETE_LIMIT).await {
            Ok(ran) if ran.output.contains(&format!("{MARK} deleted")) => {
                format!("Deleted {shown} on {name}.")
            }
            Ok(ran) if ran.output.contains(&format!("{MARK} missing")) => {
                format!("{shown} was not on {name}.")
            }
            Ok(ran) => kept(ran.output.trim()),
            Err(e) => kept(&format!("{e:#}")),
        }
    }

    /// Whether the node on `machine` no longer syncs `project`: waits for the sync status the
    /// node pushes to drop it.
    async fn stopped_syncing(&self, machine: EndpointId, project: &str) -> bool {
        let Ok(client) = self.peers.client(machine).await else {
            return false;
        };
        let dropped = client.snapshot_until(Read::SyncList, |value| match value {
            Value::Sync(reply) => match reply.as_ref() {
                Reply::Status { projects } => projects
                    .iter()
                    .all(|status| status.name != project)
                    .then_some(()),
                _ => None,
            },
            _ => None,
        });

        matches!(timeout(STOP_WAIT, dropped).await, Ok(Ok(())))
    }
}

/// Why `folder` is not deleted, or `None` when it may be: a disk's root, the home folder or a
/// Syncthing share (`roots`), or a folder that holds or lies in another project's (`others`).
fn why_kept(folder: &str, roots: &[&str], others: &[String]) -> Option<String> {
    let folder = normal(folder);

    if folder.matches('/').count() < 2 {
        return Some("it is too near the root of the disk.".to_owned());
    }

    if roots.iter().any(|root| normal(root) == folder) {
        return Some("it is the home folder or a shared folder.".to_owned());
    }

    if let Some(other) = others.iter().find(|other| {
        let other = normal(other);
        within(&other, &folder) || within(&folder, &other)
    }) {
        return Some(format!("another project lives in {}.", plain_path(other)));
    }
    None
}

/// `path` without the Windows prefix, with `/` between parts and no trailing one, lower case on
/// Windows, where case does not tell folders apart.
pub(crate) fn normal(path: &str) -> String {
    let plain = plain_path(path);
    let windows = plain.contains('\\') || plain.as_bytes().get(1) == Some(&b':');
    let slashed = plain.replace('\\', "/");
    let trimmed = slashed.trim_end_matches('/');
    if windows {
        trimmed.to_lowercase()
    } else {
        trimmed.to_owned()
    }
}

/// Whether `path` is `folder` or lies in it.
pub(crate) fn within(path: &str, folder: &str) -> bool {
    path == folder
        || path
            .strip_prefix(folder)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
#[path = "tests/left_folder.rs"]
mod tests;
