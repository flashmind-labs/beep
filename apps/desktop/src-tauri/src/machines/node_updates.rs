//! Updates machines of the mesh that run an older beep-node than the newest release, as
//! they are seen online. With auto-update off, a notification says so instead; its Update
//! button updates them all and a click opens Machines. Entry point: `follow`.

use crate::store::lock;
use crate::{
    beep::state::Prefs,
    error::DesktopError,
    fleet::{Fleet, files},
    platform::notify,
};
use beep_mesh::Device;
use futures::{StreamExt, stream::FuturesUnordered};
use iroh::EndpointId;
use semver::Version;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{AppHandle, Manager};
use tokio::time::MissedTickBehavior;

/// Long enough after launch for the machines to say their versions.
const FIRST_CHECK: Duration = Duration::from_secs(2 * 60);
/// As often as the newest release is looked up.
const CHECK_EVERY: Duration = Duration::from_secs(60 * 60);

/// Updates machines that are behind as they are seen online, and as each new release
/// comes out. With auto-update off, announces each new beep-node release once instead.
pub(crate) async fn follow(app: AppHandle) {
    let fleet = app.state::<Arc<Fleet>>().inner().clone();
    let mut changes = fleet.changes();
    let mut checks =
        tokio::time::interval_at(tokio::time::Instant::now() + FIRST_CHECK, CHECK_EVERY);
    checks.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let asked = Arc::new(Mutex::new(Asked::default()));
    let mut announced: Option<Version> = None;

    loop {
        let release_check = tokio::select! {
            _ = checks.tick() => true,
            more = changes.changed() => {
                if !more {
                    return;
                }

                false
            }
        };

        if app.state::<Prefs>().node_auto_update() {
            update_each(&app, &fleet, &asked).await;
        } else if release_check
            && let Some((latest, behind)) = behind(&app).await
            && announced.as_ref() != Some(&latest)
        {
            let names: Vec<&str> = behind.iter().map(|device| device.name.as_str()).collect();
            notify::update_nodes(
                &app,
                &format!("beep-node {latest} is ready"),
                &format!("Older on {}.", names.join(", ")),
            );
            announced = Some(latest);
        }
    }
}

/// What the auto-update asked of each machine.
#[derive(Default)]
struct Asked {
    /// Machines with an update in flight: one at a time for each.
    busy: HashSet<EndpointId>,
    /// Machines the node took the request of for a release, or that another desktop asked,
    /// so it is not asked again.
    done: HashSet<(EndpointId, Version)>,
    /// Machines whose request failed for a release: asked again once the machine has been
    /// seen offline, not on every change.
    failed: HashSet<(EndpointId, Version)>,
}

impl Asked {
    /// Marks `machine` as being updated to `latest`; false when it already is or was.
    fn claim(&mut self, machine: EndpointId, latest: &Version) -> bool {
        let key = (machine, latest.clone());

        !self.done.contains(&key) && !self.failed.contains(&key) && self.busy.insert(machine)
    }

    /// The request for `machine` ended: `taken` when the node has the update under way.
    fn finish(&mut self, machine: EndpointId, latest: &Version, taken: bool) {
        self.busy.remove(&machine);

        let key = (machine, latest.clone());

        if taken {
            self.done.insert(key);
        } else {
            self.failed.insert(key);
        }
    }

    /// `behind` are the machines online and behind now: a failed one that is not among them
    /// went offline, and is asked again when it returns.
    fn seen(&mut self, behind: &HashSet<EndpointId>) {
        self.failed.retain(|(machine, _)| behind.contains(machine));
    }
}

/// Asks each machine that is behind and not already asked to update. The node waits for its
/// agents to finish their turns, at most 30 minutes, before it restarts.
async fn update_each(app: &AppHandle, fleet: &Arc<Fleet>, asked: &Arc<Mutex<Asked>>) {
    let found = behind(app).await;
    let online: HashSet<EndpointId> = found
        .as_ref()
        .map(|(_, behind)| behind.iter().map(|device| device.id).collect())
        .unwrap_or_default();
    lock(asked).seen(&online);

    let Some((latest, behind)) = found else {
        return;
    };
    let channel = app.state::<Prefs>().node_release_channel();

    for device in behind {
        let claimed = lock(asked).claim(device.id, &latest);

        if !claimed {
            continue;
        }

        let (fleet, asked, latest) = (fleet.clone(), asked.clone(), latest.clone());
        tauri::async_runtime::spawn(async move {
            let updated = files::update_machine(&fleet, device.id, false, channel).await;

            if let Err(e) = &updated {
                log::info!("updating {} to beep-node {latest}: {e}", device.name);
            }

            lock(&asked).finish(
                device.id,
                &latest,
                updated.as_ref().map_or_else(running_already, |_| true),
            );
        });
    }
}

/// Whether the node refused because an update is already under way on it, which another
/// desktop asked: it is taken.
fn running_already(e: &DesktopError) -> bool {
    e.to_string().contains(ALREADY_RUNNING)
}

/// What the node says when an update is under way (`machine::update::remote`).
const ALREADY_RUNNING: &str = "already running";

/// Updates every machine still behind, as the notification's Update button
/// asks. Each restarts once its agent sessions are idle.
// Only macOS notifications have the button.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) async fn update_behind(app: AppHandle) {
    let Some((latest, behind)) = behind(&app).await else {
        return;
    };
    let fleet = app.state::<Arc<Fleet>>().inner().clone();
    let channel = app.state::<Prefs>().node_release_channel();

    let failed: Vec<String> = behind
        .iter()
        .map(|device| {
            let fleet = &fleet;
            async move {
                files::update_machine(fleet, device.id, false, channel)
                    .await
                    .map_err(|e| format!("{}: {e}", device.name))
            }
        })
        .collect::<FuturesUnordered<_>>()
        .filter_map(|updated| async { updated.err() })
        .collect()
        .await;

    if failed.is_empty() {
        notify::text(
            &app,
            &format!("Installed beep-node {latest}"),
            "Each machine restarts once its agent sessions are idle.",
        );
    } else {
        notify::text(
            &app,
            &format!("Could not install beep-node {latest}"),
            &failed.join("\n"),
        );
    }
}

/// The newest beep-node release and the machines that run an older one,
/// when there are any.
async fn behind(app: &AppHandle) -> Option<(Version, Vec<Device>)> {
    let fleet = app.state::<Arc<Fleet>>();
    let latest = fleet
        .releases()
        .latest_node(app.state::<Prefs>().node_release_channel())
        .await?;
    let latest = Version::parse(&latest)
        .inspect_err(|e| log::info!("reading beep-node release {latest}: {e}"))
        .ok()?;
    let behind = fleet.status().behind(&latest);

    (!behind.is_empty()).then_some((latest, behind))
}

#[cfg(test)]
#[path = "../tests/machines_node_updates.rs"]
mod tests;
