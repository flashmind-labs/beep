//! Asking the node running from a folder how it stands, on its control socket.

use crate::status::{LocalStatus, MESH_SOCKET, STATUS_LINE};
use anyhow::{Context, Result, bail};
use std::{path::Path, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, split},
    time::timeout,
};

/// What the node running from `dir` says of itself; `None` when no node
/// runs from there.
pub async fn local_status(dir: &Path) -> Result<Option<LocalStatus>> {
    use crate::run::local_socket;
    /// How long the node may take to answer.
    const STATUS_LIMIT: Duration = Duration::from_secs(20);
    /// The most bytes read: far more than a status takes.
    const MAX_STATUS: u64 = 4 * 1024 * 1024;
    let Ok(stream) = local_socket::connect(&dir.join(MESH_SOCKET)).await else {
        return Ok(None);
    };
    let (read, mut write) = split(stream);
    write.write_all(format!("{STATUS_LINE}\n").as_bytes()).await?;
    let mut bytes = Vec::new();
    timeout(STATUS_LIMIT, read.take(MAX_STATUS).read_to_end(&mut bytes))
        .await
        .context("the node took too long to answer")??;
    if bytes.is_empty() {
        bail!(
            "the running node is older than this beep-node and cannot say; restart it on \
             this version (`beep-node update`, or restart the service)"
        );
    }

    let status = beep_proto::lenient::from_slice(&bytes).context("reading the node's status")?;
    Ok(Some(status))
}
