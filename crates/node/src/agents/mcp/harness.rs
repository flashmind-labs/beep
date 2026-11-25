//! Model discovery for coding agents, on this node or a peer.

use crate::agents::machine_list::find;
use crate::peer_stream::over_the_mesh;
use crate::agents::reply::{fail, json_of, parse};
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
    route(offers, request).await.unwrap_or_else(fail)
}

async fn route(offers: &Offers, request: &Request) -> Result<Reply, String> {
    let args = Args::try_from(request)?;
    let Some(name) = args.machine else {
        return Ok(answer(&offers.agents, request).await);
    };
    let mesh = offers
        .mesh
        .get()
        .ok_or("This machine is not in a mesh yet. Leave out `machine` to query here.")?;
    let target = find(&mesh, &name)
        .ok_or_else(|| format!("No machine called {name}. Use list_machines for the names."))?;

    if target.id == mesh.me() {
        return Ok(answer(&offers.agents, request).await);
    }

    over_the_mesh(&mesh, target.id, request)
        .await
        .map_err(|e| format!("Could not query {}: {e:#}", target.name))
}

/// Answers here, including when a peer sent the request. Never forwards
/// a peer's request to another machine.
pub(crate) async fn answer(agents: &Agents, request: &Request) -> Reply {
    let args = match Args::try_from(request) {
        Ok(args) => args,
        Err(e) => return fail(e),
    };
    let Some(models) = agents.models(&args.harness).await else {
        let kinds = agents.kinds();
        let names: Vec<&str> = kinds.iter().map(|kind| kind.name.as_str()).collect();
        return fail(format!(
            "This machine has no {} agent. It has: {}.",
            args.harness,
            names.join(", ")
        ));
    };

    json_of(&HarnessInfo::new(args.harness, models))
}

#[cfg(test)]
#[path = "../../tests/agents_mcp_harness.rs"]
mod tests;
