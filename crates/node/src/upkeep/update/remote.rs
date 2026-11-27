//! `update/1`: an update asked for from Beep.

use super::{
    http::{base, client},
    reexec,
    release::{Newest, download, newest, said},
    types::Error,
};
use crate::{
    agents::Reopen,
    lock,
    machine::service::{self, Restart},
    protocol::{Protocol, Session},
};
use beep_proto::node::{
    LinkRx, LinkTx,
    update::{self, ReleaseChannel},
};
use futures::FutureExt;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering as AtomicOrdering},
    },
    time::Duration,
};
use tokio::task::spawn_blocking;
use tokio_util::task::AbortOnDropHandle;

/// Set while an update from Beep runs, until its restart, so a second one
/// is refused.
static UPDATING: AtomicBool = AtomicBool::new(false);

/// How long an update waits for the agent sessions to be idle before
/// restarting anyway, in minutes. New prompts still start meanwhile, and a
/// turn cut off resumes after the restart.
pub const WAIT_MINUTES: u64 = 30;

/// The restart an update from Beep waits to run.
type Waiting = Arc<Mutex<Option<AbortOnDropHandle<()>>>>;

/// `update/1`: what `beep-node update` does, asked by the caller. Only the
/// CLI's node offers it.
pub struct Updates {
    reopen: Arc<Reopen>,
    /// Outlives the connection that asked for it.
    waiting: Waiting,
}

impl Updates {
    pub fn new(reopen: Arc<Reopen>) -> Self {
        Self {
            reopen,
            waiting: Waiting::default(),
        }
    }
}

impl Protocol for Updates {
    fn name(&self) -> &str {
        update::PROTOCOL
    }

    fn serve(&self, tx: LinkTx, rx: LinkRx) -> Session {
        Box::pin(serve(tx, rx, self.reopen.clone(), self.waiting.clone()))
    }
}

async fn serve(
    mut tx: LinkTx,
    mut rx: LinkRx,
    reopen: Arc<Reopen>,
    waiting: Waiting,
) -> anyhow::Result<()> {
    let Some(request) = rx.recv::<update::Request>().await? else {
        return Ok(());
    };
    let (now, channel) = match request {
        update::Request::Update { now } => (now, ReleaseChannel::All),
        update::Request::Minor { now } => (now, ReleaseChannel::Minor),
        _ => anyhow::bail!("unsupported node update request"),
    };

    if UPDATING.swap(true, AtomicOrdering::SeqCst) {
        tx.send(&update::Event::Failed {
            reason: "An update is already running on this machine.".into(),
        })
        .await?;
        return tx.finish().await;
    }

    let (event, restart) = match remote(&reopen, now, channel).await {
        Ok(Remote { event, restart }) => (event, restart),
        Err(e) => {
            // With its causes, as one line.
            let reason = format!("{:#}", anyhow::Error::new(e));
            log::warn!("update failed: {reason}");
            (update::Event::Failed { reason }, None)
        }
    };
    let Some(next) = restart else {
        UPDATING.store(false, AtomicOrdering::SeqCst);
        tx.send(&event).await?;
        return tx.finish().await;
    };
    // The answer reaches the caller before the restart drops the link. The
    // binary is installed, so it restarts even when the caller is gone.
    let answered = async {
        tx.send(&event).await?;
        tx.finish().await
    };

    if let Err(e) = answered.await {
        log::debug!("the caller did not read the update's answer: {e:#}");
    }

    let limit = Duration::from_secs(if now { 0 } else { WAIT_MINUTES * 60 });
    let restart = tokio::spawn(async move {
        let restarted = reopen
            .restart_when_idle(limit, async {
                match next {
                    Next::Service(restart) => {
                        log::info!("restarting the service on the new binary");
                        spawn_blocking(move || restart.run().map(drop)).await
                    }
                    Next::Itself(exe) => {
                        log::info!("restarting on the new binary");
                        reexec::ask(exe);
                        Ok(Ok(()))
                    }
                }
            })
            .await;
        match restarted {
            Ok(Ok(())) => {}
            Ok(Err(e)) => log::warn!("the service did not restart: {e:#}"),
            Err(e) => log::warn!("the restart failed: {e}"),
        }

        UPDATING.store(false, AtomicOrdering::SeqCst);
    });
    *lock(&waiting) = Some(AbortOnDropHandle::new(restart));
    Ok(())
}

/// What an update from Beep did, and the restart to run.
struct Remote {
    event: update::Event,
    restart: Option<Next>,
}

/// What starts the new binary.
#[derive(Debug)]
enum Next {
    /// The service that runs this node.
    Service(Restart),
    /// This node, in place, having no service: a container, or a node run
    /// in the foreground.
    Itself(PathBuf),
}

/// Whether the service restarts the node, or it starts the new binary
/// itself. `None` leaves it on the old one: on Windows a node with no
/// service of its own keeps running until it is restarted.
fn next(restart: Restart, exe: PathBuf, unix: bool) -> Option<Next> {
    match restart {
        Restart::Restarted(_) => Some(Next::Service(restart)),
        // The service is stopped, runs another binary or is not there, so
        // this node is not its: it runs `exe` and restarts itself.
        _ if unix => Some(Next::Itself(exe)),
        _ => None,
    }
}

async fn remote(reopen: &Reopen, now: bool, channel: ReleaseChannel) -> Result<Remote, Error> {
    let base = base();
    let client = client()?;
    let Newest { current, latest } = newest(&client, &base, channel).await?;

    if current >= latest {
        return Ok(Remote {
            event: update::Event::Newest {
                version: current.to_string(),
            },
            restart: None,
        });
    }

    log::info!("updating beep-node {current} to {latest}");
    let exe = download(&client, &base, &latest).await?.install().await?;
    let restart = {
        let exe = exe.clone();
        spawn_blocking(move || service::restart_plan(&exe)).await?
    }
    .map_err(|source| Error::Restart {
        version: latest.clone(),
        source,
    })?;
    let not_restarting = said(&restart, &latest);
    let next = next(restart, exe, cfg!(unix));
    let note = if next.is_none() {
        not_restarting
    } else if !now && reopen.until_idle().now_or_never().is_none() {
        format!(
            "New prompts can still start. It restarts when sessions are idle, or after {WAIT_MINUTES} minutes."
        )
    } else {
        String::new()
    };
    Ok(Remote {
        event: update::Event::Installed {
            from: current.to_string(),
            to: latest.to_string(),
            restarting: next.is_some(),
            note,
        },
        restart: next,
    })
}

#[cfg(test)]
#[path = "../../tests/upkeep_update_remote.rs"]
mod tests;
