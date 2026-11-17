//! A notification when a newer beep-node is out and machines of the mesh
//! run an older one. Its Update button updates them all; a click opens
//! Machines. Entry point: `follow`.

use crate::{
    beep::state::Prefs,
    fleet::{Fleet, files},
    notices::notify,
};
use beep_mesh::Device;
use futures::{StreamExt, stream::FuturesUnordered};
use semver::Version;
use std::{sync::Arc, time::Duration};
use tauri::{AppHandle, Manager};

/// Long enough after launch for the machines to say their versions.
const FIRST_CHECK: Duration = Duration::from_secs(2 * 60);
/// As often as the newest release is looked up.
const CHECK_EVERY: Duration = Duration::from_secs(60 * 60);

/// Announces each new beep-node release once while machines run an older one.
pub(crate) async fn follow(app: AppHandle) {
    tokio::time::sleep(FIRST_CHECK).await;
    let mut announced: Option<Version> = None;

    loop {
        if let Some((latest, behind)) = behind(&app).await
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

        tokio::time::sleep(CHECK_EVERY).await;
    }
}

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
