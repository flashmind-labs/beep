//! `wsl/1` (see `beep_proto::node::wsl`): adds a WSL distribution as a
//! machine, or stops running one.

use super::{distro::DistroName, keeper::Keeper, steps};
use crate::{
    link::{Offers, Protocol, Session},
    machine::env,
};
use anyhow::{Context, Result, anyhow, bail};
use beep_proto::{
    SecretText,
    node::{
        LinkRx, LinkTx,
        wsl::{Event, PROTOCOL, Request},
    },
};
use std::{path::Path, sync::Arc};

/// Serves `wsl/1` and keeps the WSL machines running.
pub struct Wsl(Arc<Keeper>);

impl Wsl {
    /// Starts the WSL machines saved in the node's folder `dir`. Needs a
    /// running Tokio runtime.
    pub fn start(dir: &Path, offers: &Offers) -> Self {
        Self(Keeper::start(dir, offers.mesh.clone()))
    }
}

impl Protocol for Wsl {
    fn name(&self) -> &str {
        PROTOCOL
    }

    fn serve(&self, tx: LinkTx, rx: LinkRx) -> Session {
        Box::pin(serve(tx, rx, self.0.clone()))
    }
}

async fn serve(mut tx: LinkTx, mut rx: LinkRx, keeper: Arc<Keeper>) -> Result<()> {
    let Some(request) = rx.recv::<Request>().await? else {
        return Ok(());
    };
    let done = match request {
        Request::Add { distro, ticket } => add(&keeper, &distro, &ticket, &mut tx).await,
        Request::Stop { distro } => stop(&keeper, &distro).await,
        _ => Err(anyhow!("Update Beep on this machine first.")),
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

/// Installs beep-node in `distro`, keeps it running and joins it with
/// `ticket`, telling the caller each step. A node that could not join is
/// not kept.
async fn add(keeper: &Keeper, distro: &str, ticket: &SecretText, tx: &mut LinkTx) -> Result<()> {
    let distro: DistroName = distro.parse()?;
    let listed = steps::list(false)
        .await
        .context("could not list the WSL distributions")?;

    if !listed.contains(&distro) {
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

async fn stop(keeper: &Keeper, distro: &str) -> Result<()> {
    let distro: DistroName = distro.parse()?;
    keeper.stop(&distro).await
}

/// Tells the caller a step began. A caller gone does not stop the work.
async fn step(tx: &mut LinkTx, text: String) {
    log::info!("WSL: {text}");

    if let Err(e) = tx.send(&Event::Step { text }).await {
        log::debug!("the caller did not hear a WSL step: {e:#}");
    }
}
