//! The machine's WSL distributions, for its report. The routine report
//! looks into the running distributions only, as looking into a stopped one
//! starts it; `probe_all` looks into every one when the user asks.

use super::{
    distro::{DistroName, Found, FoundAgent},
    keeper, steps,
};
use anyhow::Result;
use beep_proto::node::WslDistro;
use futures::future::join_all;
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
    time::Duration,
};
use tokio::{sync::Notify, time::timeout};

/// How long the routine report waits for the running distributions' probes.
const PROBES_LIMIT: Duration = Duration::from_secs(10);
/// How long looking into every distribution may take, starting them.
const PROBE_ALL_LIMIT: Duration = Duration::from_secs(60);

/// What each distribution had when it was last probed well.
static FOUND: LazyLock<Mutex<HashMap<DistroName, Found>>> = LazyLock::new(Mutex::default);
/// Told when a probe finds other agents than before.
static AGENTS_CHANGED: Notify = Notify::const_new();

/// The coding agent `name` installed in a distribution, the first by name
/// when several have it, as last probed.
pub(crate) fn agent(name: &str) -> Option<(DistroName, FoundAgent)> {
    let found = crate::lock(&FOUND);
    let mut with: Vec<(&DistroName, &FoundAgent)> = found
        .iter()
        .filter_map(|(distro, found)| Some((distro, found.agents.iter().find(|agent| agent.name == name)?)))
        .collect();
    with.sort_by_key(|(distro, _)| *distro);

    with.first().map(|(distro, agent)| ((*distro).clone(), (*agent).clone()))
}

/// Waits until a probe finds other agents in a distribution than before.
pub(crate) async fn agents_changed() {
    AGENTS_CHANGED.notified().await;
}

/// The machine's WSL distributions, with what each had when last probed
/// and how its node stands; empty when WSL is not installed.
pub(crate) async fn report() -> Vec<WslDistro> {
    let listed = match steps::list().await {
        Ok(listed) => listed,
        Err(e) => {
            log::debug!("no WSL distributions: {e:#}");
            return Vec::new();
        }
    };
    let running: Vec<&DistroName> = listed
        .iter()
        .filter(|listed| listed.running)
        .map(|listed| &listed.name)
        .collect();
    probe(&running, PROBES_LIMIT).await;

    let found = crate::lock(&FOUND).clone();
    let mut report: Vec<WslDistro> = listed
        .iter()
        .map(|listed| described(&listed.name, found.get(&listed.name).cloned()))
        .collect();
    // A kept distribution WSL no longer lists, until the keeper drops it.
    let missing = keeper::running()
        .map(|keeper| keeper.kept())
        .unwrap_or_default()
        .into_iter()
        .filter(|kept| !listed.iter().any(|listed| listed.name == *kept));

    for distro in missing {
        let mut gone = described(&distro, None);
        gone.problem = Some(format!("There is no WSL distribution {distro} any more."));
        report.push(gone);
    }

    report
}

/// Looks into every distribution, starting the stopped ones.
pub(crate) async fn probe_all() -> Result<()> {
    let listed = steps::list().await?;
    let names: Vec<&DistroName> = listed.iter().map(|listed| &listed.name).collect();
    probe(&names, PROBE_ALL_LIMIT).await;
    Ok(())
}

/// Probes `distros` together for at most `limit`, keeping what each probe
/// that worked found. A failed one keeps what was found before.
async fn probe(distros: &[&DistroName], limit: Duration) {
    let probes = distros.iter().map(|distro| async move {
        match steps::probe(distro).await {
            Ok(found) => {
                let agents = found.agents.clone();
                let before = crate::lock(&FOUND).insert((*distro).clone(), found);

                if before.is_none_or(|before| before.agents != agents) {
                    AGENTS_CHANGED.notify_one();
                }
            }
            Err(e) => log::debug!("probing WSL distribution {distro} failed: {e:#}"),
        }
    });

    if timeout(limit, join_all(probes)).await.is_err() {
        log::debug!("probing the WSL distributions took over {} s", limit.as_secs());
    }
}

/// `distro` with what it was found to have, if it was probed, and how the
/// keeper stands with it.
fn described(distro: &DistroName, found: Option<Found>) -> WslDistro {
    let keeper = keeper::running();
    let standing = keeper.and_then(|keeper| keeper.standing(distro));
    let probed = found.is_some();
    let found = found.unwrap_or_default();

    if let (Some(keeper), Some(id)) = (keeper, &found.machine_id)
        && let Ok(id) = id.parse()
    {
        keeper.know(distro, id);
    }

    WslDistro {
        name: distro.to_string(),
        agents: found.agents.into_iter().map(|agent| agent.name).collect(),
        node: found.node,
        machine_id: found.machine_id,
        kept: standing.is_some(),
        problem: standing.and_then(|standing| standing.problem),
        probed,
    }
}
