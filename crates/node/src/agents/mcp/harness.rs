//! Model discovery for coding agents, on this node or a peer.

use super::machines::{find, over_the_mesh};
use super::reply::{fail as failed, ok, parse};
use crate::{
    agents::{Agents, known},
    link::Offers,
};
use beep_proto::node::{
    agent::{Effort, ModelList, ModelOption, ModelPick, efforts},
    tools::{Reply, Request},
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    harness: String,
    machine: Option<String>,
}

impl TryFrom<&Request> for Args {
    type Error = String;

    fn try_from(request: &Request) -> Result<Self, Self::Error> {
        let mut args: Self = parse(&request.args)?;
        args.harness = args.harness.trim().to_owned();

        if args.harness.is_empty() {
            return Err("Give a `harness`, as list_machines names it.".to_owned());
        }

        Ok(args)
    }
}

#[derive(Serialize)]
struct HarnessInfo {
    harness: String,
    title: String,
    model_selection: ModelPick,
    default_model: Option<String>,
    models: Vec<ModelOption>,
    effort_levels: &'static [Effort],
}

impl HarnessInfo {
    fn new(harness: String, models: ModelList) -> Self {
        let title = known(&harness)
            .map_or(harness.as_str(), |kind| kind.title)
            .to_owned();
        let effort_levels = efforts(&harness);
        Self {
            harness,
            title,
            model_selection: models.pick,
            default_model: models.current,
            models: models.models,
            effort_levels,
        }
    }
}

/// Routes discovery to the named machine, defaulting to this node even
/// when it has not joined a mesh.
pub(super) async fn call(offers: &Offers, request: &Request) -> Reply {
    let args = match Args::try_from(request) {
        Ok(args) => args,
        Err(e) => return failed(e),
    };
    let Some(name) = args.machine else {
        return answer(&offers.agents, request).await;
    };
    let Some(mesh) = offers.mesh.get() else {
        return failed("This machine is not in a mesh yet. Leave out `machine` to query here.");
    };
    let Some(target) = find(&mesh, &name) else {
        return failed(format!(
            "No machine called {name}. Use list_machines for the names."
        ));
    };

    if target.id == mesh.me() {
        return answer(&offers.agents, request).await;
    }

    match over_the_mesh(&mesh, target.id, request).await {
        Ok(reply) => reply,
        Err(e) => failed(format!("Could not query {}: {e:#}", target.name)),
    }
}

/// Answers here, including when a peer sent the request. Never forwards
/// a peer's request to another machine.
pub(crate) async fn answer(agents: &Agents, request: &Request) -> Reply {
    let args = match Args::try_from(request) {
        Ok(args) => args,
        Err(e) => return failed(e),
    };
    if let Err(refused) = agents.harness_or_refusal(&args.harness) {
        return failed(refused);
    }

    let Some(models) = agents.models(&args.harness).await else {
        return failed(agents.no_agent(&args.harness));
    };

    match serde_json::to_string(&HarnessInfo::new(args.harness, models)) {
        Ok(text) => ok(text),
        Err(e) => failed(format!("Could not write the harness info: {e}")),
    }
}

#[cfg(test)]
#[path = "../../tests/agents_mcp_harness.rs"]
mod tests;
