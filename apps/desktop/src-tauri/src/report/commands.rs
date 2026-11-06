//! The commands the Report a problem sheet calls: collect a draft, send it,
//! and open the GitHub issue with the user's own `gh`. Sending signs with
//! the device key and needs no server link.

use crate::{
    beep::Service,
    error::{DesktopError, Result},
    fleet::Fleet,
    machines::this_mac::Machine,
    settings::cli_login::find_tool,
};
use anyhow::Context;
use beep_mesh::{Device, DeviceKind, PeerClient};
use beep_report::{
    Draft, Manifest, Period, Sender, Target, collect, draft, find_gh, gh_signed_in, issue,
    open_issue, redact, send, server_url, zip,
};
use chrono::Local;
use futures::future::join_all;
use iroh::EndpointId;
use serde::Serialize;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tauri::State;
use tokio::time::timeout;

/// How long a machine has to be reached before it is listed offline.
const REACH_LIMIT: Duration = Duration::from_secs(3);

/// A sent report, as the sheet reads it.
#[derive(Debug, Serialize)]
pub(crate) struct Sent {
    reference: String,
    /// Whether `gh` is here and signed in, so the sheet offers the issue.
    gh: bool,
}

/// Every machine's share of the report for `period`, redacted, with no text
/// yet: the text is added at send time.
#[tauri::command]
pub async fn report_collect(
    fleet: State<'_, Arc<Fleet>>,
    machine: State<'_, Machine>,
    period: Period,
) -> Result<Draft> {
    let since = period.since(Local::now());
    let me = fleet.id();
    let mut devices: Vec<Device> = fleet
        .machines()
        .into_iter()
        .map(|status| status.device)
        .collect();
    // Until the mesh opens it lists nobody, this Mac included.
    if devices.is_empty() {
        devices.push(Device {
            id: me,
            name: machine.name().to_owned(),
            kind: DeviceKind::Desktop,
            added_at_ms: 0,
        });
    }
    let fleet: &Fleet = &fleet;
    let targets = join_all(devices.into_iter().map(|device| async move {
        let client = reach(fleet, device.id, &device.name).await;
        Target {
            id: device.id,
            name: device.name,
            kind: device.kind,
            client,
        }
    }))
    .await;
    let shares = collect(targets, since).await;
    let sender = Sender {
        id: me,
        mesh: fleet.mesh().map(|mesh| mesh.id()),
        app: "desktop",
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    Ok(draft("", period, since, sender, shares).map_err(anyhow::Error::from)?)
}

/// A client for `id` within `REACH_LIMIT`, or why there is none.
async fn reach(fleet: &Fleet, id: EndpointId, name: &str) -> Result<PeerClient, String> {
    match timeout(REACH_LIMIT, fleet.client(id)).await {
        Ok(Ok(client)) => Ok(client),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err(DesktopError::Offline(name.to_owned()).to_string()),
    }
}

/// Sends `draft` with `text`, signed by this Mac's key, straight to the
/// report endpoint.
#[tauri::command]
pub async fn report_send(beep: Service<'_>, draft: Draft, text: String) -> Result<Sent> {
    let mut draft = draft;
    draft.manifest.text = redact(&text).map_err(anyhow::Error::from)?;
    let body = zip(&draft).map_err(anyhow::Error::from)?;
    let url = server_url().map_err(anyhow::Error::from)?;
    let receipt = send(&url, body, &beep.device_key())
        .await
        .map_err(anyhow::Error::from)?;
    Ok(Sent {
        reference: receipt.reference,
        gh: signed_in(gh()).await.is_some(),
    })
}

/// Opens the public issue for a sent report and returns its URL.
#[tauri::command]
pub async fn report_issue(manifest: Manifest, reference: String) -> Result<String> {
    let gh = gh().ok_or_else(|| DesktopError::BadRequest("gh is not installed".into()))?;
    let issue = issue(&manifest, &reference).map_err(anyhow::Error::from)?;
    Ok(open_issue(&gh, &issue)
        .await
        .context("the report was sent, but the GitHub issue did not open")?)
}

/// The issue's title and body, as `report_issue` would open it.
#[tauri::command]
pub fn report_issue_text(manifest: Manifest, reference: String) -> Result<String> {
    let issue = issue(&manifest, &reference).map_err(anyhow::Error::from)?;
    Ok(format!("{}\n\n{}", issue.title, issue.body))
}

/// `gh` on the login `PATH` or in the usual tool folders.
fn gh() -> Option<PathBuf> {
    find_tool("gh").or_else(find_gh)
}

/// `gh` when it is signed in to GitHub.
async fn signed_in(gh: Option<PathBuf>) -> Option<PathBuf> {
    let gh = gh?;
    gh_signed_in(&gh).await.then_some(gh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn sent_says_gh_only_when_signed_in() {
        use std::{fs, os::unix::fs::PermissionsExt};
        let dir = tempfile::tempdir().expect("a temp dir is made");
        let stand_in = |name: &str, status: u8| {
            let path = dir.path().join(name);
            fs::write(&path, format!("#!/bin/sh\nexit {status}\n")).expect("the script is written");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
                .expect("the script is made executable");
            path
        };
        let out = stand_in("gh-out", 1);
        let ready = stand_in("gh-in", 0);
        assert_eq!(signed_in(Some(out)).await, None);
        assert_eq!(signed_in(Some(ready.clone())).await, Some(ready));
        assert_eq!(signed_in(None).await, None);
    }
}
