//! Interactive terminals on a node (`terminal/1`).

use super::Online;
use crate::{
    protocol::node::{
        LinkRx, LinkTx,
        process::TaskId,
        terminal::{PROTOCOL, Reply, Request, Size},
    },
    store::TerminalRow,
};
use anyhow::{Result, bail};
use beep_mesh::{
    PeerClient,
    machine::terminals::{self, Kept},
};

impl Online {
    fn client(&self) -> PeerClient {
        PeerClient::new(self.connection.clone())
    }

    /// Sends `request` to the node's terminals and returns its reply.
    pub async fn terminals(&self, request: &Request) -> Result<Reply> {
        if !self.offers(PROTOCOL) {
            bail!(
                "{} runs no interactive terminals; it needs a newer beep-node",
                self.name
            );
        }
        Ok(terminals::request(&self.client(), request).await?)
    }

    /// Opens a stream following terminal `id` from byte `from`.
    pub(crate) async fn attach_terminal(
        &self,
        id: TaskId,
        from: u64,
        size: Size,
    ) -> Result<(LinkTx, LinkRx)> {
        Ok(terminals::attach(&self.client(), id, from, size).await?)
    }

    /// Stops terminal `row` if it still runs its command; one reusing its number is left alone.
    pub(crate) async fn stop_terminal(&self, row: &TerminalRow) -> Result<()> {
        let kept = Kept {
            task: row.task,
            pty: row.pty,
            command: &row.command,
        };
        if !row.pty {
            // Background tasks go through the server's own retrying link.
            return self.stop_task(TaskId(row.task), &row.command).await;
        }
        Ok(terminals::stop(&self.client(), &kept).await?)
    }
}
