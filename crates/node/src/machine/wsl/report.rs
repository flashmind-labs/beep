//! The machine's WSL distributions, for its report.

use super::{
    distro::{DistroName, Found},
    keeper, steps,
};
use beep_proto::node::WslDistro;
use futures::future::join_all;
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
};

/// What each distribution had when last probed. A distribution that is not
/// running is probed once, as probing starts it; a running one each time.
static FOUND: LazyLock<Mutex<HashMap<DistroName, Found>>> = LazyLock::new(Mutex::default);

/// The machine's WSL distributions, with what each has and how its node
/// stands; empty when WSL is not installed.
pub(crate) async fn report() -> Vec<WslDistro> {
    let distros = match steps::list(false).await {
        Ok(distros) => distros,
        Err(e) => {
            log::debug!("no WSL distributions: {e:#}");
            return Vec::new();
        }
    };
    let running = steps::list(true).await.unwrap_or_default();
    let running = &running;
    let probes = distros.iter().map(|distro| async move {
        let known = crate::lock(&FOUND).get(distro).cloned();
        let found = match known {
            Some(found) if !running.contains(distro) => found,
            known => match steps::probe(distro).await {
                Ok(found) => found,
                Err(e) => {
                    log::debug!("probing WSL distribution {distro} failed: {e:#}");
                    known.unwrap_or_default()
                }
            },
        };
        crate::lock(&FOUND).insert(distro.clone(), found.clone());
        described(distro, found)
    });

    join_all(probes).await
}

/// `distro` with what it has, and how the keeper stands with it.
fn described(distro: &DistroName, found: Found) -> WslDistro {
    let keeper = keeper::running();
    let standing = keeper.and_then(|keeper| keeper.standing(distro));

    if let (Some(keeper), Some(id)) = (keeper, &found.machine_id)
        && let Ok(id) = id.parse()
    {
        keeper.know(distro, id);
    }

    WslDistro {
        name: distro.to_string(),
        agents: found.agents,
        node: found.node,
        machine_id: found.machine_id,
        kept: standing.is_some(),
        problem: standing.and_then(|standing| standing.problem),
    }
}
