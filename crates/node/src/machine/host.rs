//! What the machine calls itself.

use beep_proto::link::MAX_NAME;

/// The name when the host has none to give.
const NO_HOSTNAME: &str = "Machine";

/// What the machine calls itself, cut to fit a name.
pub async fn host_name() -> String {
    let output = crate::run::process::command("hostname").output().await;
    let name = output
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default();
    let name = name.split('.').next().unwrap_or_default();

    if name.is_empty() || name.len() > MAX_NAME {
        return NO_HOSTNAME.to_owned();
    }

    name.to_owned()
}
