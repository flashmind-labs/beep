//! Hook events from Claude Code's socket, passed to every `Watch` stream
//! through a broadcast.

use super::{
    hooks::{HookEvent, HookMessage},
    live::Live,
};
use crate::run::local_socket::{self as socket, Listener, Stream};
use anyhow::{Context, Result};
use beep_proto::node::agent::Activity;
use std::{path::Path, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::broadcast,
    time::timeout,
};

/// Hook events waiting for slow streams.
const HUB_SIZE: usize = 64;
/// How long a hook has to send its line.
const HOOK_READ: Duration = Duration::from_secs(2);
const MAX_HOOK_LINE: u64 = 64 * 1024;

/// Hands hook events to every stream that follows sessions.
pub(crate) struct Hub(broadcast::Sender<Activity>);

impl Default for Hub {
    fn default() -> Self {
        Self(broadcast::channel(HUB_SIZE).0)
    }
}

impl Hub {
    /// The hook events from now on.
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<Activity> {
        self.0.subscribe()
    }

    fn send(&self, activity: Activity) {
        // Nobody follows anything now; the event is not needed.
        let _ = self.0.send(activity);
    }
}

/// Takes hook events on `socket` until the node stops, one connection at a
/// time, each a single line answered with what the hook prints.
pub(crate) async fn listen(socket: &Path, hub: &Hub, live: &Live) -> Result<()> {
    let mut listener = Listener::bind(socket)?;
    log::info!("taking Claude Code hooks on {}", socket::shown(socket));

    loop {
        let stream = listener.accept().await?;

        match timeout(HOOK_READ, read_hook(stream, live)).await {
            Ok(Ok(Some(activity))) => {
                log::debug!(
                    "hook: session {} {}",
                    activity.session,
                    activity.kind.as_ref()
                );
                crate::events::system().changed(beep_proto::node::events::Topic::keyed(
                    "agent",
                    &activity.session,
                ));
                crate::events::system().changed(beep_proto::node::events::Topic::new("agents"));
                hub.send(activity);
            }
            Ok(Ok(None)) => {}
            Ok(Err(e)) => log::debug!("skipped a hook: {e:#}"),
            Err(_) => log::debug!("a hook sent nothing in time"),
        }
    }
}

async fn read_hook(mut stream: Stream, live: &Live) -> Result<Option<Activity>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut line = String::new();
    BufReader::new((&mut stream).take(MAX_HOOK_LINE))
        .read_line(&mut line)
        .await?;
    let message: HookMessage = serde_json::from_str(&line).context("not a hook message")?;
    let answer = live.hear(&message);

    if let Some(answer) = &answer {
        stream.write_all(answer.to_string().as_bytes()).await?;
        stream.shutdown().await?;
    }
    // A blocked stop goes on with the turn, so the turn did not end.
    if message.event == HookEvent::Stop && answer.is_some() {
        return Ok(None);
    }

    Ok(message.activity())
}

#[cfg(test)]
#[path = "../../tests/occupancy_claude_watch.rs"]
mod tests;
