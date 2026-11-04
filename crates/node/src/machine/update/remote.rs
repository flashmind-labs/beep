//! `update/1`: an update asked for from Beep.

use super::{
    Error, Newest, download,
    http::{base, client},
    newest, said,
};
use crate::{
    agents::Agents,
    link::{Protocol, Session},
    lock,
    machine::service::{self, Restart},
};
use beep_proto::node::{LinkRx, LinkTx, update};
use futures::FutureExt;
use std::{
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
/// restarting anyway, in minutes.
pub const WAIT_MINUTES: u64 = 30;

/// The restart an update from Beep waits to run.
type Waiting = Arc<Mutex<Option<AbortOnDropHandle<()>>>>;

/// `update/1`: what `beep-node update` does, asked by the server. Only the
/// CLI's node offers it.
pub struct Updates {
    agents: Arc<Agents>,
    /// Outlives the connection that asked for it.
    waiting: Waiting,
}

impl Updates {
    pub fn new(agents: Arc<Agents>) -> Self {
        Self {
            agents,
            waiting: Waiting::default(),
        }
    }
}

impl Protocol for Updates {
    fn name(&self) -> &str {
        update::PROTOCOL
    }

    fn serve(&self, tx: LinkTx, rx: LinkRx) -> Session {
        Box::pin(serve(tx, rx, self.agents.clone(), self.waiting.clone()))
    }
}

async fn serve(
    mut tx: LinkTx,
    mut rx: LinkRx,
    agents: Arc<Agents>,
    waiting: Waiting,
) -> anyhow::Result<()> {
    let Some(update::Request::Update { now }) = rx.recv::<update::Request>().await? else {
        return Ok(());
    };
    if UPDATING.swap(true, AtomicOrdering::SeqCst) {
        tx.send(&update::Event::Failed {
            reason: "An update is already running on this machine.".into(),
        })
        .await?;
        return tx.finish().await;
    }
    let (event, restart) = match remote(&agents, now).await {
        Ok(Remote { event, restart }) => (event, restart),
        Err(e) => {
            // With its causes, as one line.
            let reason = format!("{:#}", anyhow::Error::new(e));
            log::warn!("update failed: {reason}");
            (update::Event::Failed { reason }, None)
        }
    };
    let Some(plan) = restart else {
        UPDATING.store(false, AtomicOrdering::SeqCst);
        tx.send(&event).await?;
        return tx.finish().await;
    };
    // The answer reaches the server before the restart drops the link. The
    // binary is installed, so it restarts even when the server is gone.
    let answered = async {
        tx.send(&event).await?;
        tx.finish().await
    };
    if let Err(e) = answered.await {
        log::debug!("the server did not read the update's answer: {e:#}");
    }
    let limit = Duration::from_secs(if now { 0 } else { WAIT_MINUTES * 60 });
    let restart = tokio::spawn(async move {
        let restarted = agents
            .restart_when_idle(limit, async {
                log::info!("restarting the service on the new binary");
                spawn_blocking(move || plan.run()).await
            })
            .await;
        match restarted {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => log::warn!("the service did not restart: {e:#}"),
            Err(e) => log::warn!("the restart failed: {e}"),
        }
        UPDATING.store(false, AtomicOrdering::SeqCst);
    });
    *lock(&waiting) = Some(AbortOnDropHandle::new(restart));
    Ok(())
}

/// What an update from Beep did, and the restart to run when the service
/// should.
struct Remote {
    event: update::Event,
    restart: Option<service::Plan>,
}

async fn remote(agents: &Agents, now: bool) -> Result<Remote, Error> {
    let base = base();
    let client = client()?;
    let Newest { current, latest } = newest(&client, &base).await?;
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
    let plan = {
        let exe = exe.clone();
        spawn_blocking(move || service::restart_plan(&exe)).await?
    }
    .map_err(|source| Error::Restart {
        version: latest.clone(),
        source,
    })?;
    let restarting = matches!(plan.restart, Restart::Restarted(_));
    let note = if !restarting {
        said(&plan.restart, &latest)
    } else if !now && agents.until_idle().now_or_never().is_none() {
        "It restarts once its agent sessions are idle.".to_owned()
    } else {
        String::new()
    };
    Ok(Remote {
        event: update::Event::Installed {
            from: current.to_string(),
            to: latest.to_string(),
            restarting,
            note,
        },
        restart: restarting.then_some(plan),
    })
}
