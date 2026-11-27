//! What adb does for an Android device, phone or emulator, by its adb
//! serial: one call at a time, and the logs, screen contents, installs,
//! launches and resets the agents ask for.

use crate::{machine::env, process};
use anyhow::{Context, Result, bail};
use beep_proto::node::device::{DeviceId, Element};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::time::timeout;

pub(super) const NO_ADB: &str = "adb is not installed on this machine.";
/// How long one adb call may take; a screenshot over USB 2 takes about a
/// second.
pub(super) const CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// How long an install may take: a large APK over USB 2.
const INSTALL_TIMEOUT: Duration = Duration::from_secs(300);
/// How many of the newest logcat lines a filter looks through.
const LOGCAT_LINES: &str = "20000";

/// The newest lines of device `id`'s log.
pub(super) async fn logcat(id: &DeviceId) -> Result<String> {
    let printed = run(id, &["logcat", "-d", "-t", LOGCAT_LINES]).await?;

    Ok(String::from_utf8_lossy(&printed).into_owned())
}

/// What device `id`'s screen shows, from `uiautomator`.
pub(super) async fn ui(id: &DeviceId) -> Result<Vec<Element>> {
    let printed = run(id, &["exec-out", "uiautomator", "dump", "/dev/tty"]).await?;

    elements(&String::from_utf8_lossy(&printed))
}

/// Installs the APK at `path` here, replacing the app if it is there.
pub(super) async fn install(id: &DeviceId, path: &Path) -> Result<()> {
    let adb = env::adb().context(NO_ADB)?;
    let path = path.to_str().context("the path is not text")?;
    let printed = call(&adb, id, &["install", "-r", "-d", path], INSTALL_TIMEOUT).await?;
    let said = String::from_utf8_lossy(&printed);

    if !said.contains("Success") {
        bail!("adb install: {}", said.trim());
    }

    Ok(())
}

/// Starts package `target`, or opens it when it is a URL.
pub(super) async fn launch(id: &DeviceId, target: &str) -> Result<()> {
    let command = if target.contains("://") {
        let quoted = target.replace('\'', r"'\''");
        format!("am start -a android.intent.action.VIEW -d '{quoted}'")
    } else {
        let package = package(target)?;
        format!("monkey -p {package} -c android.intent.category.LAUNCHER 1")
    };
    let printed = run(id, &["shell", &command]).await?;
    let said = String::from_utf8_lossy(&printed);

    if said.contains("No activities found") || said.contains("Error:") {
        bail!("{} did not start: {}", target, said.trim());
    }

    Ok(())
}

/// Stops package `app` and clears its data.
pub(super) async fn reset(id: &DeviceId, app: &str) -> Result<()> {
    let package = package(app)?;
    let printed = run(
        id,
        &[
            "shell",
            &format!("am force-stop {package} && pm clear {package}"),
        ],
    )
    .await?;
    let said = String::from_utf8_lossy(&printed);

    if !said.contains("Success") {
        bail!("{package} was not cleared: {}", said.trim());
    }

    Ok(())
}

/// `call` with the adb here and the usual time limit.
async fn run(id: &DeviceId, args: &[&str]) -> Result<Vec<u8>> {
    let adb = env::adb().context(NO_ADB)?;
    call(&adb, id, args, CALL_TIMEOUT).await
}

/// What `adb -s id <args>` printed, once it succeeded within `limit`.
pub(super) async fn call(
    adb: &Path,
    id: &DeviceId,
    args: &[&str],
    limit: Duration,
) -> Result<Vec<u8>> {
    let mut command = process::command(adb);
    command
        .arg("-s")
        .arg(&id.0)
        .args(args)
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let output = timeout(limit, command.output())
        .await
        .context("adb did not answer")?;
    process::finished(adb, &output);
    let output = output.context("could not run adb")?;

    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }

    Ok(output.stdout)
}

/// `name` when it reads as an Android package, such as `com.example.app`.
fn package(name: &str) -> Result<&str> {
    let valid = !name.is_empty()
        && name.contains('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_');

    if !valid {
        bail!("{name:?} is no Android package name, such as com.example.app.");
    }

    Ok(name)
}

/// The elements of a `uiautomator dump` that say something or have an id,
/// placed as fractions of the screen, which the first node covers.
pub(super) fn elements(dumped: &str) -> Result<Vec<Element>> {
    let start = dumped.find("<?xml").context("uiautomator gave no screen")?;
    let end = dumped
        .rfind("</hierarchy>")
        .context("uiautomator gave no screen")?;
    let xml = &dumped[start..end + "</hierarchy>".len()];
    let tree = roxmltree::Document::parse(xml).context("uiautomator's screen did not read")?;
    let nodes: Vec<_> = tree
        .descendants()
        .filter(|node| node.has_tag_name("node"))
        .collect();
    let screen = nodes
        .first()
        .and_then(|node| bounds(node.attribute("bounds")?))
        .context("uiautomator gave no screen size")?;
    let (width, height) = (screen.2.max(1.0), screen.3.max(1.0));

    Ok(nodes
        .iter()
        .filter_map(|node| {
            let text = node.attribute("text").unwrap_or_default();
            let label = if text.is_empty() {
                node.attribute("content-desc").unwrap_or_default()
            } else {
                text
            };
            let id = node.attribute("resource-id").unwrap_or_default();
            let id = id.split_once(":id/").map_or(id, |(_, id)| id);

            if label.is_empty() && id.is_empty() {
                return None;
            }

            let (left, top, right, bottom) = bounds(node.attribute("bounds")?)?;

            if right <= left || bottom <= top {
                return None;
            }

            let class = node.attribute("class").unwrap_or_default();
            Some(Element {
                label: label.to_owned(),
                kind: class.rsplit('.').next().unwrap_or(class).to_owned(),
                id: id.to_owned(),
                x: left / width,
                y: top / height,
                width: (right - left) / width,
                height: (bottom - top) / height,
            })
        })
        .collect())
}

/// `[left,top][right,bottom]`, as uiautomator writes a node's bounds.
fn bounds(text: &str) -> Option<(f32, f32, f32, f32)> {
    let numbers: Vec<f32> = text
        .split(|c: char| !c.is_ascii_digit() && c != '-')
        .filter(|part| !part.is_empty())
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let [left, top, right, bottom] = numbers[..] else {
        return None;
    };

    Some((left, top, right, bottom))
}

#[cfg(test)]
#[path = "../../tests/run_device_adb.rs"]
mod tests;
