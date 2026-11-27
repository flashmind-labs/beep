//! The machine's WSL distributions, for its report. The routine report
//! looks into the running distributions, as looking into a stopped one
//! starts it, and into the default one once in the background.

use super::{
    distro::{DistroName, Found, Listed},
    steps,
};
use beep_proto::node::WslDistro;
use futures::future::join_all;
use std::{
    collections::HashMap,
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{sync::Notify, time::timeout};

/// How long the routine report waits for the running distributions' probes.
const PROBES_LIMIT: Duration = Duration::from_secs(10);
/// How long looking into the stopped default distribution may take, starting it.
const DEFAULT_PROBE_LIMIT: Duration = Duration::from_secs(60);

/// What each distribution had when it was last probed well.
static FOUND: LazyLock<Mutex<HashMap<DistroName, Found>>> = LazyLock::new(Mutex::default);
/// The distribution `wsl.exe` starts by default, as last listed.
static DEFAULT: Mutex<Option<DistroName>> = Mutex::new(None);
/// Told when a probe finds other agents than before.
static AGENTS_CHANGED: Notify = Notify::const_new();
/// The default distribution was looked into once while stopped.
static DEFAULT_PROBED: AtomicBool = AtomicBool::new(false);

/// What each distribution had when last probed, the default one first,
/// then by name.
pub(crate) fn found() -> Vec<(DistroName, Found)> {
    let default = crate::lock(&DEFAULT).clone();
    let mut found: Vec<(DistroName, Found)> = crate::lock(&FOUND)
        .iter()
        .map(|(distro, found)| (distro.clone(), found.clone()))
        .collect();
    found.sort_by(|(a, _), (b, _)| {
        (default.as_ref() != Some(a), a).cmp(&(default.as_ref() != Some(b), b))
    });
    found
}

/// The distribution that has the coding agent `name`, with what it has,
/// as `found` orders them.
pub(crate) fn with_agent(name: &str) -> Option<(DistroName, Found)> {
    found()
        .into_iter()
        .find(|(_, found)| found.agents.iter().any(|agent| agent.name == name))
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
    let default = listed.iter().find(|listed| listed.default);
    *crate::lock(&DEFAULT) = default.map(|listed| listed.name.clone());
    forget_gone(&listed);

    // The default distribution is looked into once even when stopped, so
    // the agents in it are found without the user starting it. Starting it
    // can take longer than the report waits.
    if let Some(default) = default
        && !default.running
        && !DEFAULT_PROBED.swap(true, Ordering::Relaxed)
    {
        let name = default.name.clone();
        tokio::spawn(async move { probe(&[&name], DEFAULT_PROBE_LIMIT).await });
    }

    let running: Vec<&DistroName> = listed
        .iter()
        .filter(|listed| listed.running)
        .map(|listed| &listed.name)
        .collect();
    probe(&running, PROBES_LIMIT).await;

    let found = crate::lock(&FOUND).clone();
    listed
        .iter()
        .map(|listed| described(&listed.name, found.get(&listed.name)))
        .collect()
}

/// Forgets what was found in the distributions WSL no longer lists.
fn forget_gone(listed: &[Listed]) {
    let mut found = crate::lock(&FOUND);
    let before = found.len();
    found.retain(|name, _| listed.iter().any(|listed| listed.name == *name));

    if found.len() != before {
        AGENTS_CHANGED.notify_one();
    }
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
        log::debug!(
            "probing the WSL distributions took over {} s",
            limit.as_secs()
        );
    }
}

/// `distro` with the agents it was found to have, if it was probed. The
/// fields of the machines distributions once ran as stay empty.
fn described(distro: &DistroName, found: Option<&Found>) -> WslDistro {
    WslDistro {
        name: distro.to_string(),
        agents: found
            .map(|found| {
                found
                    .agents
                    .iter()
                    .map(|agent| agent.name.clone())
                    .collect()
            })
            .unwrap_or_default(),
        node: None,
        machine_id: None,
        kept: false,
        problem: None,
        probed: found.is_some(),
    }
}
