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
use anyhow::{Context, Result, bail};
use std::time::Duration;
use tokio::time::timeout;

/// How long one request may take; a stop waits for the terminal to end.
const LIMIT: Duration = Duration::from_secs(20);

impl Online {
    /// Sends `request` to the node's terminals and returns its reply.
    pub async fn terminals(&self, request: &Request) -> Result<Reply> {
        if !self.offers(PROTOCOL) {
            bail!(
                "{} runs no interactive terminals; it needs a newer beep-node",
                self.name
            );
        }
        let (mut tx, mut rx) = self.open_stream(PROTOCOL).await?;
        tx.send(request).await?;
        timeout(LIMIT, rx.recv::<Reply>())
            .await
            .context("the node stopped answering")??
            .context("the node closed the terminal session")
    }

    /// Opens a stream following terminal `id` from byte `from`.
    pub(crate) async fn attach_terminal(
        &self,
        id: TaskId,
        from: u64,
        size: Size,
    ) -> Result<(LinkTx, LinkRx)> {
        let (mut tx, rx) = self.open_stream(PROTOCOL).await?;
        tx.send(&Request::Attach { id, from }).await?;
        tx.send(&Request::Resize { size }).await?;
        Ok((tx, rx))
    }

    /// Stops terminal `row` if it still runs its command; one reusing its number is left alone.
    pub(crate) async fn stop_terminal(&self, row: &TerminalRow) -> Result<()> {
        let id = TaskId(row.task);
        if !row.pty {
            return self.stop_task(id, &row.command).await;
        }
        let Reply::Terminals { terminals } = self.terminals(&Request::List {}).await? else {
            bail!("the node did not list its terminals");
        };
        let running = terminals
            .iter()
            .any(|task| task.id == id && task.command == row.command && task.state.is_running());
        if !running {
            return Ok(());
        }
        match self.terminals(&Request::Stop { id }).await? {
            Reply::Error { message } => bail!("{message}"),
            _ => Ok(()),
        }
    }
}
