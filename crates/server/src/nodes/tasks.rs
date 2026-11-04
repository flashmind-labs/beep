//! Background commands on a node (`process/1`), one stream per call.

use super::{NODE_BACK, Online, Repeat, retry::Unsent};
use crate::protocol::node::process::{PROTOCOL, Reply, Request, TaskId};
use anyhow::{Context, Result, bail};
use std::time::Duration;
use tokio::time::{Instant, timeout};

/// How long one request may take; a stop waits for the group to end.
const LIMIT: Duration = Duration::from_secs(20);

impl Online {
    /// Sends `request` to the node's background tasks and returns its
    /// reply. When the link drops, it goes again once the node is back,
    /// unless it starts a task the node may have started.
    pub async fn tasks(&self, request: &Request) -> Result<Reply> {
        if !self.offers(PROTOCOL) {
            bail!(
                "{} runs no background commands; it needs a newer beep-node",
                self.name
            );
        }
        let repeat = Repeat::when(!matches!(request, Request::Start { .. }));
        let until = Instant::now() + NODE_BACK;
        let mut on = self.clone();
        loop {
            let error = match on.tasks_once(request).await {
                Ok(reply) => return Ok(reply),
                Err(error) => error,
            };
            match on.again(&error, repeat, until).await {
                Some(back) => on = back,
                None => return Err(error),
            }
        }
    }

    async fn tasks_once(&self, request: &Request) -> Result<Reply> {
        let (mut tx, mut rx) = self.open_stream(PROTOCOL).await?;
        tx.send(request).await.map_err(Unsent)?;
        timeout(LIMIT, rx.recv::<Reply>())
            .await
            .context("the node stopped answering")??
            .context("the node closed the process session")
    }

    /// Stops task `id` if it still runs `command`; one reusing its number is left alone.
    pub(crate) async fn stop_task(&self, id: TaskId, command: &str) -> Result<()> {
        match self.tasks(&Request::Status { id }).await? {
            Reply::Status { task, .. } if task.command == command && task.state.is_running() => {
                match self.tasks(&Request::Stop { id }).await? {
                    Reply::Error { message } => bail!("{message}"),
                    _ => Ok(()),
                }
            }
            _ => Ok(()),
        }
    }
}
