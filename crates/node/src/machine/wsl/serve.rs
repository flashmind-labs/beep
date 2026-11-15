//! `wsl/1` (see `beep_proto::node::wsl`): adds a WSL distribution as a
//! machine, turns its node off or on, and looks into the distributions.

use super::{
    distro::DistroName,
    keeper::{self, Keeper},
    report, steps,
};
use crate::{
    link::{Offers, Protocol, Session, from_mesh::Membership},
    machine::env,
};
use anyhow::{Context, Result, anyhow, bail};
use beep_proto::{
    SecretText, lenient,
    node::{
        LinkRx, LinkTx,
        wsl::{Event, PROTOCOL, Request},
    },
};
use std::{
    convert::Infallible,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Serves `wsl/1`, and runs the WSL machines while the node serves as a
/// machine itself.
pub struct Wsl {
    /// The node's folder, which saves the WSL machines.
    dir: PathBuf,
    mesh: Membership,
}

impl Wsl {
    /// The WSL machines saved in the node's folder `dir`, not running yet.
    pub fn new(dir: &Path, offers: &Offers) -> Self {
        Self {
            dir: dir.to_owned(),
            mesh: offers.mesh.clone(),
        }
    }

    /// Runs the WSL machines while the node serves as a machine (`lent`),
    /// else stops them. Only one process runs them: the one whose node
    /// serves. Needs a running Tokio runtime.
    pub async fn lend(&self, lent: bool) {
        if lent {
            Keeper::start(&self.dir, self.mesh.clone());
        } else if let Some(keeper) = keeper::running() {
            keeper.halt().await;
        }

        env::changed();
    }
}

impl Protocol for Wsl {
    fn name(&self) -> &str {
        PROTOCOL
    }

    fn serve(&self, tx: LinkTx, rx: LinkRx) -> Session {
        Box::pin(serve(tx, rx))
    }
}

async fn serve(mut tx: LinkTx, mut rx: LinkRx) -> Result<()> {
    // Read apart, so a request from a newer Beep gets an answer.
    let read = |bytes: &[u8]| Ok::<_, Infallible>(lenient::from_slice::<Request>(bytes));
    let Some(request) = rx.recv_with(read).await? else {
        return Ok(());
    };
    let done = match (request, keeper::running()) {
        (Err(_), _) => Err(anyhow!("Update Beep on this machine first.")),
        (Ok(_), None) => Err(anyhow!(
            "This computer runs no WSL machines while Beep may not use it."
        )),
        (Ok(request), Some(keeper)) => answer(request, keeper, &mut tx).await,
    };
    // Its report says how the distributions stand now.
    env::changed();

    let event = match done {
        Ok(()) => Event::Done {},
        Err(e) => {
            let reason = format!("{e:#}");
            log::warn!("WSL: {reason}");
            Event::Failed { reason }
        }
    };
    tx.send(&event).await?;
    tx.finish().await
}

async fn answer(request: Request, keeper: &Arc<Keeper>, tx: &mut LinkTx) -> Result<()> {
    match request {
        Request::Add { distro, ticket } => add(keeper, &distro, &ticket, tx).await,
        Request::Stop { distro } => keeper.stop(&distro.parse()?).await,
        Request::Start { distro } => keeper.keep(&distro.parse()?),
        Request::Probe {} => report::probe_all().await,
        _ => Err(anyhow!("Update Beep on this machine first.")),
    }
}

/// Installs beep-node in `distro`, keeps it running and joins it with
/// `ticket`, telling the caller each step. A node that could not join is
/// not kept.
async fn add(keeper: &Arc<Keeper>, distro: &str, ticket: &SecretText, tx: &mut LinkTx) -> Result<()> {
    let distro: DistroName = distro.parse()?;
    let listed = steps::list()
        .await
        .context("could not list the WSL distributions")?;

    if !listed.iter().any(|listed| listed.name == distro) {
        bail!("This machine has no WSL distribution named {distro}.");
    }

    step(tx, format!("Installing beep-node in {distro}")).await;
    steps::install(&distro)
        .await
        .with_context(|| format!("Could not install beep-node in {distro}"))?;
    step(tx, format!("Starting beep-node in {distro}")).await;
    keeper.keep(&distro)?;
    step(tx, format!("Joining {distro} to your machines")).await;

    if let Err(e) = steps::join(&distro, ticket.expose()).await {
        let problem = keeper.standing(&distro).and_then(|standing| standing.problem);

        if let Err(stop) = keeper.stop(&distro).await {
            log::warn!("could not stop the node in {distro}: {stop:#}");
        }

        let why = problem.map(|problem| format!(" ({problem})"));
        return Err(e.context(format!(
            "{distro} could not join{}",
            why.unwrap_or_default()
        )));
    }

    match steps::probe(&distro).await {
        Ok(found) => {
            if let Some(id) = found.machine_id.and_then(|id| id.parse().ok()) {
                keeper.know(&distro, id);
            }
        }
        Err(e) => log::debug!("probing {distro} after it joined failed: {e:#}"),
    }

    Ok(())
}

/// Tells the caller a step began. A caller gone does not stop the work.
async fn step(tx: &mut LinkTx, text: String) {
    log::info!("WSL: {text}");

    if let Err(e) = tx.send(&Event::Step { text }).await {
        log::debug!("the caller did not hear a WSL step: {e:#}");
    }
}
