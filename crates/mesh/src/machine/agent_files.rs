//! Detect the shell used by a peer machine.

use super::exec;
use crate::{MeshError, PeerClient};
use std::time::Duration;

/// Whether the machine's shell is PowerShell on Windows.
pub async fn on_windows(client: &PeerClient) -> Result<bool, MeshError> {
    let ran = exec(client, "echo $env:OS", Duration::from_secs(20)).await?;
    Ok(ran.output.trim() == "Windows_NT")
}
