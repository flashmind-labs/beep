//! What the machine calls itself.

use beep_proto::MAX_MACHINE_NAME;

/// The name when the host has none to give.
const NO_HOSTNAME: &str = "Machine";

/// What the machine calls itself, cut to fit a name. In WSL, whose host
/// name is the Windows machine's, the distribution comes first, as
/// "Ubuntu on desk-pc".
pub async fn host_name() -> String {
    let output = crate::process::command("hostname").output().await;
    let name = output
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default();

    match std::env::var("WSL_DISTRO_NAME") {
        Ok(distro) if !distro.is_empty() => cut(&in_wsl(&distro, &name)),
        _ => short_name(&name),
    }
}

/// "Ubuntu on desk-pc" for distribution `distro` on Windows host `host`.
fn in_wsl(distro: &str, host: &str) -> String {
    let host = host.split('.').next().unwrap_or_default();

    if host.is_empty() {
        return distro.to_owned();
    }

    format!("{distro} on {host}")
}

/// The first label of `host`, cut to fit a name.
fn short_name(host: &str) -> String {
    cut(host.split('.').next().unwrap_or_default())
}

/// `name` cut to `MAX_MACHINE_NAME` bytes at a character boundary;
/// `NO_HOSTNAME` when that is empty.
fn cut(name: &str) -> String {
    let name = &name[..name.floor_char_boundary(MAX_MACHINE_NAME)];

    if name.is_empty() {
        return NO_HOSTNAME.to_owned();
    }

    name.to_owned()
}

#[cfg(test)]
#[path = "../tests/machine_host.rs"]
mod tests;
