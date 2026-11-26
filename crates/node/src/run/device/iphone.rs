//! iPhones and iPads plugged into this Mac, through `xcrun devicectl`:
//! listed, shown from one screenshot after another, and given apps to
//! install and launch. `devicectl` has no touch input, so they take none.

use super::{Controls as AnyControls, Live, Screen, phone};
use crate::process;
use anyhow::{Context, Result, bail};
use beep_proto::node::device::{Device, DeviceId, DeviceState, Platform};
use serde::Deserialize;
use std::{path::Path, process::Stdio, time::Duration};
use tokio::sync::watch;
use tokio_util::task::AbortOnDropHandle;

/// How long one `devicectl` call may take; a screenshot takes about two
/// seconds.
const CALL_TIMEOUT: Duration = Duration::from_secs(20);
/// How long an install may take: a large app over USB 2.
const INSTALL_TIMEOUT: Duration = Duration::from_secs(300);
/// Opens a URL given to `launch`.
const SAFARI: &str = "com.apple.mobilesafari";
pub(super) const NO_INPUT: &str =
    "Beep cannot tap or type on an iPhone yet: use the phone itself.";

/// The iPhones and iPads this Mac reaches.
pub(super) async fn list() -> Vec<Device> {
    let text = match devicectl(&["list", "devices", "-j", "-"], CALL_TIMEOUT).await {
        Ok(text) => text,
        Err(e) => {
            log::debug!("no iPhones: {e:#}");
            return Vec::new();
        }
    };

    match parse_devices(&text) {
        Ok(devices) => devices,
        Err(e) => {
            log::warn!("could not read the iPhones: {e:#}");
            Vec::new()
        }
    }
}

/// `devicectl list devices -j -`.
#[derive(Deserialize)]
struct Listed {
    result: Found,
}

#[derive(Deserialize)]
struct Found {
    devices: Vec<Listing>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Listing {
    connection_properties: Connection,
    device_properties: Properties,
    hardware_properties: Hardware,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Connection {
    #[serde(default)]
    pairing_state: String,
    #[serde(default)]
    tunnel_state: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Properties {
    name: String,
    #[serde(default)]
    os_version_number: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Hardware {
    #[serde(default)]
    device_type: String,
    #[serde(default)]
    reality: String,
    udid: Option<String>,
}

/// The paired iPhones and iPads `devicectl` can reach, from its JSON;
/// simulators and devices that are away are left out.
pub(super) fn parse_devices(text: &str) -> Result<Vec<Device>> {
    let listed: Listed = serde_json::from_str(text)?;
    let devices = listed
        .result
        .devices
        .into_iter()
        .filter(|device| {
            let hardware = &device.hardware_properties;
            let connection = &device.connection_properties;

            hardware.reality == "physical"
                && matches!(hardware.device_type.as_str(), "iPhone" | "iPad")
                && connection.pairing_state == "paired"
                && connection.tunnel_state != "unavailable"
        })
        .filter_map(|device| {
            let properties = device.device_properties;
            let os = (!properties.os_version_number.is_empty())
                .then(|| format!("iOS {}", properties.os_version_number));

            Some(Device {
                id: DeviceId(device.hardware_properties.udid?),
                name: properties.name,
                platform: Platform::Ios,
                state: DeviceState::On,
                os,
                phone: true,
            })
        })
        .collect();

    Ok(devices)
}

/// Installs the `.app` at `path` on iPhone `id`.
pub(super) async fn install(id: &DeviceId, path: &Path) -> Result<()> {
    let path = path.to_str().context("the path is not text")?;
    devicectl(&["device", "install", "app", "-d", &id.0, path], INSTALL_TIMEOUT).await?;

    Ok(())
}

/// Starts app `target` by its bundle id, or opens it in Safari when it is
/// a URL.
pub(super) async fn launch(id: &DeviceId, target: &str) -> Result<()> {
    let mut args = vec!["device", "process", "launch", "-d", &id.0];

    if target.contains("://") {
        args.extend(["--payload-url", target, SAFARI]);
    } else {
        args.push(target);
    }

    devicectl(&args, CALL_TIMEOUT).await?;

    Ok(())
}

/// Follows iPhone `id`'s screen, one screenshot after another.
pub(super) async fn attach(id: &DeviceId, max_side: u32) -> Result<Live> {
    let (screen, shown) = watch::channel(Screen::Starting);
    let work = tokio::spawn(capture(id.clone(), max_side, screen));

    Ok(Live {
        screen: shown,
        controls: AnyControls::Iphone,
        _work: AbortOnDropHandle::new(work),
    })
}

/// Shows iPhone `id`'s screen as fast as `devicectl` takes screenshots,
/// until it stops answering.
async fn capture(id: DeviceId, max_side: u32, screen: watch::Sender<Screen>) {
    let reason = match screenshots(&id, max_side, &screen).await {
        Ok(()) => "The iPhone stopped.".to_owned(),
        Err(e) => format!("The iPhone stopped answering ({e:#})."),
    };

    screen.send_replace(Screen::Ended(reason));
}

async fn screenshots(id: &DeviceId, max_side: u32, screen: &watch::Sender<Screen>) -> Result<()> {
    let dir = tempfile::Builder::new().prefix("beep-iphone").tempdir()?;
    let shot = dir.path().join("screen.png");
    let destination = shot.to_str().context("the folder is not text")?;
    let mut last = Vec::new();

    while !screen.is_closed() {
        let args = ["device", "capture", "screenshot", "-d", &id.0, "--destination", destination];
        devicectl(&args, CALL_TIMEOUT).await?;
        let png = tokio::fs::read(&shot).await?;

        if png != last {
            last.clone_from(&png);

            match tokio::task::spawn_blocking(move || phone::frame(&png, max_side)).await? {
                Ok((_, frame)) => {
                    screen.send_replace(Screen::Frame(frame));
                }
                Err(e) => log::warn!("an iPhone screenshot was not shown: {e:#}"),
            }
        }
    }

    Ok(())
}

/// What `xcrun devicectl args` printed; an error with what it said when it
/// failed or took longer than `limit`.
async fn devicectl(args: &[&str], limit: Duration) -> Result<String> {
    let seconds = limit.as_secs().to_string();
    let mut command = process::command("xcrun");
    command
        .arg("devicectl")
        .args(args)
        .args(["-q", "-t", &seconds])
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(limit + Duration::from_secs(5), command.output())
        .await
        .with_context(|| format!("devicectl {} took too long", args.join(" ")))?;
    process::finished("xcrun", &output);
    let output = output.context("could not run xcrun devicectl")?;

    if !output.status.success() {
        let said = String::from_utf8_lossy(&output.stderr);
        bail!("devicectl {}: {}", args.join(" "), said.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
#[path = "../../tests/run_device_iphone.rs"]
mod tests;
