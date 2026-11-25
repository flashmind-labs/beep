//! The tools of each machine that have a newer release: what its node found, which a node that
//! pushes its list gives and one that predates the topic is asked for, and its coding agents
//! behind their newest npm release.

use super::{Fleet, ToolPush};
use beep_proto::{
    Machine, ToolUpdate,
    node::{
        events::Topic,
        harness::{harness, is_newer, parse_version},
    },
};
use futures::{StreamExt, stream};
use iroh::EndpointId;
use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

/// How long the newest coding agent releases found are kept.
const RELEASES_FOR: Duration = Duration::from_secs(55 * 60);
/// How long one npm registry lookup may take.
const NPM_TIMEOUT: Duration = Duration::from_secs(15);

/// The newest npm release of each coding agent package, `None` for one the lookup missed, and
/// when they were looked up.
static AGENT_RELEASES: Mutex<Option<(Instant, HashMap<&'static str, Option<String>>)>> =
    Mutex::new(None);

/// The updates found by each connected node, and its coding agents behind
/// their newest release. A node that pushes its list is not asked; one that
/// predates the topic answers the check call.
pub(crate) async fn all(fleet: &Arc<Fleet>) -> Vec<ToolUpdate> {
    let machines: Vec<Machine> = fleet
        .status()
        .machines()
        .into_iter()
        .filter(|machine| machine.online)
        .collect();
    let (tools, coders) = tokio::join!(
        node_updates(fleet, machines.clone()),
        agent_updates(&machines)
    );

    let mut updates: Vec<_> = tools.into_iter().chain(coders).collect();
    updates.sort_by(|a, b| (&a.machine_id, &a.tool).cmp(&(&b.machine_id, &b.tool)));
    updates
}

/// The `gh`, `teacli` and `flsh` updates each node found.
async fn node_updates(fleet: &Arc<Fleet>, machines: Vec<Machine>) -> Vec<ToolUpdate> {
    stream::iter(machines)
        .map(|machine| async move {
            let id = fleet.member(&machine.id)?;

            match fleet.projections().tool_pushes.get(id) {
                Some(ToolPush::Pushed(found)) => Some(
                    found
                        .into_iter()
                        .map(|update| ToolUpdate {
                            machine_id: machine.id.clone(),
                            tool: update.tool.to_string(),
                            version: update.version,
                            latest: update.latest,
                        })
                        .collect(),
                ),
                // Its first list is on its way.
                None => None,
                Some(ToolPush::Refused) => Some(asked_updates(fleet, id, machine.id).await),
            }
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .flatten()
        .flatten()
        .collect()
}

/// The updates a node that does not push them found, from its check call.
async fn asked_updates(fleet: &Arc<Fleet>, id: EndpointId, machine_id: String) -> Vec<ToolUpdate> {
    let fleet_fetch = fleet.clone();

    fleet
        .projections()
        .tool_updates
        .read_or_default(
            id,
            Vec::new,
            PartialEq::eq,
            move || beep_node::events::system().changed(Topic::new("tools")),
            move || async move {
                let client = fleet_fetch.client(id).await?;
                let updates = beep_mesh::machine::tool_updates(&client).await?;
                Ok(updates
                    .into_iter()
                    .map(|update| ToolUpdate {
                        machine_id: machine_id.clone(),
                        tool: update.tool.to_string(),
                        version: update.version,
                        latest: update.latest,
                    })
                    .collect())
            },
        )
        .await
}

/// A coding agent a machine reported, with its npm package.
struct Coder<'a> {
    machine: &'a Machine,
    agent: &'a str,
    package: &'static str,
    version: String,
}

/// The coding agents on `machines` behind their newest npm release, from
/// the versions each machine reported.
async fn agent_updates(machines: &[Machine]) -> Vec<ToolUpdate> {
    let coders: Vec<Coder<'_>> = machines
        .iter()
        .flat_map(|machine| {
            let tools = machine.env.iter().flat_map(|env| &env.tools);
            tools.filter_map(move |tool| {
                Some(Coder {
                    machine,
                    agent: &tool.name,
                    package: harness(&tool.name)?.npm?,
                    version: parse_version(tool.version.as_deref()?)?,
                })
            })
        })
        .collect();
    let packages: BTreeSet<&'static str> = coders.iter().map(|coder| coder.package).collect();
    let releases = agent_releases(&packages).await;

    coders
        .into_iter()
        .filter_map(|coder| {
            let latest = releases.get(coder.package)?.as_ref()?;
            is_newer(latest, &coder.version).then(|| ToolUpdate {
                machine_id: coder.machine.id.clone(),
                tool: coder.agent.to_owned(),
                version: coder.version,
                latest: latest.clone(),
            })
        })
        .collect()
}

/// The newest release of each of `packages`, looked up again when the last look is old. A
/// package the look missed stays missed until then.
async fn agent_releases(
    packages: &BTreeSet<&'static str>,
) -> HashMap<&'static str, Option<String>> {
    {
        let found = AGENT_RELEASES
            .lock()
            .unwrap_or_else(PoisonError::into_inner);

        if let Some((at, releases)) = found.as_ref()
            && at.elapsed() < RELEASES_FOR
            && packages
                .iter()
                .all(|package| releases.contains_key(package))
        {
            return releases.clone();
        }
    }

    let Ok(http) = beep_proto::https::client().timeout(NPM_TIMEOUT).build() else {
        return HashMap::new();
    };
    let looked = futures::future::join_all(packages.iter().map(|&package| {
        let http = http.clone();
        async move {
            let url = format!(
                "https://registry.npmjs.org/{}/latest",
                package.replace('/', "%2F")
            );
            let latest = async {
                let response = http.get(url).send().await.ok()?.error_for_status().ok()?;
                let metadata: serde_json::Value =
                    serde_json::from_str(&response.text().await.ok()?).ok()?;
                parse_version(metadata.get("version")?.as_str()?)
            };
            (package, latest.await)
        }
    }))
    .await;

    let mut found = AGENT_RELEASES
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let (at, releases) = found.get_or_insert_with(|| (Instant::now(), HashMap::new()));
    *at = Instant::now();

    for (package, latest) in looked {
        // A miss does not replace a release found before.
        match latest {
            Some(latest) => {
                releases.insert(package, Some(latest));
            }
            None => {
                releases.entry(package).or_insert(None);
            }
        }
    }

    releases.clone()
}

#[cfg(test)]
#[path = "../tests/fleet_tool_updates.rs"]
mod tests;
