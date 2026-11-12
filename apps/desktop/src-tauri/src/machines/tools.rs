//! Newer `gh`, `teacli`, `flsh` and coding agents on the account's
//! machines, and installing them.

use crate::{
    beep::Service,
    error::{DesktopError, Result},
    fleet::{Fleet, Machines, agents, files as direct},
};
use beep_proto::{
    Machine, ToolUpdate, UpdateMachineTool,
    node::{
        harness::{harness, is_newer, parse_version},
        tool_update::Tool,
    },
};
use futures::{StreamExt, stream};
use iroh::EndpointId;
use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};
use tauri::State;

/// How long a machine has to download and install a release.
const UPDATE_LIMIT: Duration = Duration::from_secs(11 * 60);
/// How long the newest coding agent releases found are kept.
const RELEASES_FOR: Duration = Duration::from_secs(55 * 60);
/// How long one npm registry lookup may take.
const NPM_TIMEOUT: Duration = Duration::from_secs(15);

/// The newest npm release of each coding agent package, and when they were
/// looked up.
static AGENT_RELEASES: Mutex<Option<(Instant, HashMap<&'static str, String>)>> = Mutex::new(None);

/// The updates found by each connected node, and its coding agents behind
/// their newest release.
#[tauri::command]
pub async fn tool_updates(fleet: State<'_, Arc<Fleet>>) -> Result<Vec<ToolUpdate>> {
    let machines: Vec<Machine> = fleet
        .status()
        .machines()
        .into_iter()
        .filter(|machine| machine.online)
        .collect();
    let fleet = fleet.inner().as_ref();
    let (tools, coders) = tokio::join!(
        node_updates(fleet, machines.clone()),
        agent_updates(&machines)
    );

    let mut updates: Vec<_> = tools.into_iter().chain(coders).collect();
    updates.sort_by(|a, b| (&a.machine_id, &a.tool).cmp(&(&b.machine_id, &b.tool)));
    Ok(updates)
}

/// The `gh`, `teacli` and `flsh` updates each node found.
async fn node_updates(fleet: &Fleet, machines: Vec<Machine>) -> Vec<ToolUpdate> {
    stream::iter(machines)
        .map(|machine| async move {
            let Some(id) = fleet.member(&machine.id) else {
                return Vec::new();
            };
            let Ok(client) = fleet.client(id).await else {
                return Vec::new();
            };
            let Ok(updates) = beep_mesh::machine::tool_updates(&client).await else {
                return Vec::new();
            };
            updates
                .into_iter()
                .map(|update| ToolUpdate {
                    machine_id: machine.id.clone(),
                    tool: update.tool.to_string(),
                    version: update.version,
                    latest: update.latest,
                })
                .collect::<Vec<_>>()
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .flatten()
        .collect()
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
            let latest = releases.get(coder.package)?;
            is_newer(latest, &coder.version).then(|| ToolUpdate {
                machine_id: coder.machine.id.clone(),
                tool: coder.agent.to_owned(),
                version: coder.version,
                latest: latest.clone(),
            })
        })
        .collect()
}

/// The newest release of each of `packages`, looked up again when the last
/// look is old or missed one.
async fn agent_releases(packages: &BTreeSet<&'static str>) -> HashMap<&'static str, String> {
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
        if let Some(latest) = latest {
            releases.insert(package, latest);
        }
    }

    releases.clone()
}

/// Installs the newest `tool` on `machine_id`; gives the version installed.
#[tauri::command]
pub async fn update_machine_tool(
    beep: Service<'_>,
    fleet: Machines<'_>,
    machine_id: String,
    tool: String,
) -> Result<String> {
    let Some(machine) = fleet.member(&machine_id) else {
        return beep
            .call_within(UpdateMachineTool { machine_id, tool }, UPDATE_LIMIT)
            .await;
    };
    let Ok(named) = tool.parse::<Tool>() else {
        return update_agent(&fleet, machine, &tool).await;
    };
    let client = fleet.client(machine).await?;
    let latest = beep_mesh::machine::tool_updates(&client)
        .await
        .map_err(|e| DesktopError::BadRequest(format!("Could not check {named}: {e:#}")))?
        .into_iter()
        .find(|update| update.tool == named)
        .map(|update| update.latest)
        .ok_or_else(|| DesktopError::BadRequest(format!("No update found for {named}.")))?;
    direct::update_tool(&fleet, machine, named, &latest).await?;
    Ok(latest)
}

/// Updates coding agent `agent` on `machine`; gives the version it reports
/// after.
async fn update_agent(fleet: &Fleet, machine: EndpointId, agent: &str) -> Result<String> {
    if harness(agent).is_none() {
        return Err(DesktopError::BadRequest(format!(
            "Beep does not update {agent}."
        )));
    }

    agents::update_harness(fleet, machine, agent)
        .await?
        .harnesses
        .into_iter()
        .find(|item| item.agent == agent)
        .and_then(|item| item.version)
        .ok_or_else(|| {
            DesktopError::BadRequest(format!("{agent} did not say its version after the update."))
        })
}
