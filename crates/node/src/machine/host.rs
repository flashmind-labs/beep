//! What the machine calls itself.

use beep_proto::MAX_MACHINE_NAME;

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
    short_name(&name)
}

/// The first label of `host`, cut to `MAX_MACHINE_NAME` bytes at a character
/// boundary; `NO_HOSTNAME` when that is empty.
fn short_name(host: &str) -> String {
    let name = host.split('.').next().unwrap_or_default();
    let name = &name[..name.floor_char_boundary(MAX_MACHINE_NAME)];

    if name.is_empty() {
        return NO_HOSTNAME.to_owned();
    }

    name.to_owned()
}

#[cfg(test)]
#[path = "../tests/machine_host.rs"]
mod tests;
