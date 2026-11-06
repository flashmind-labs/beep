//! `update/1`: an update asked for from Beep.

use super::{
    Error, Newest, download,
    http::{base, client},
    newest, reexec, said,
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
    let Some(next) = restart else {
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
                match next {
                    Next::Service(plan) => {
                        log::info!("restarting the service on the new binary");
                        spawn_blocking(move || plan.run().map(drop)).await
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
    Service(service::Plan),
    /// This node, in place, having no service: a container, or a node run
    /// in the foreground.
    Itself(PathBuf),
}

/// Whether the service restarts the node, or it starts the new binary
/// itself. `None` leaves it on the old one: on Windows a node with no
/// service of its own keeps running until it is restarted.
fn next(plan: service::Plan, exe: PathBuf, unix: bool) -> Option<Next> {
    match plan.restart {
        Restart::Restarted(_) => Some(Next::Service(plan)),
        // The service is stopped, runs another binary or is not there, so
        // this node is not its: it runs `exe` and restarts itself.
        _ if unix => Some(Next::Itself(exe)),
        _ => None,
    }
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
    let not_restarting = said(&plan.restart, &latest);
    let next = next(plan, exe, cfg!(unix));
    let note = if next.is_none() {
        not_restarting
    } else if !now && agents.until_idle().now_or_never().is_none() {
        "It restarts once its agent sessions are idle.".to_owned()
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
mod tests {
    use super::*;
    use crate::machine::service::Manager;

    fn plan(restart: Restart) -> service::Plan {
        service::Plan::new(restart)
    }

    #[test]
    fn a_running_service_restarts_the_node() {
        let restarted = plan(Restart::Restarted(Manager::OpenRc));
        let chosen = next(restarted, PathBuf::from("/bin/beep-node"), true);
        assert!(matches!(chosen, Some(Next::Service(_))), "{chosen:?}");
    }

    #[test]
    fn a_node_with_no_service_of_its_own_restarts_itself_on_unix() {
        let exe = PathBuf::from("/bin/beep-node");
        for restart in [
            Restart::NotInstalled,
            Restart::Stopped(Manager::Systemd),
            Restart::OtherBinary(PathBuf::from("/etc/init.d/beep-node")),
        ] {
            let chosen = next(plan(restart.clone()), exe.clone(), true);
            assert!(
                matches!(&chosen, Some(Next::Itself(path)) if *path == exe),
                "{restart:?}: {chosen:?}"
            );
            assert!(next(plan(restart), exe.clone(), false).is_none());
        }
    }
}
