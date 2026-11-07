//! Model discovery for coding agents, on this node or a peer.

use super::machines::{command_name, machines, over_the_mesh};
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

impl Args {
    fn parse(request: &Request) -> Result<Self, String> {
        let mut args: Self = serde_json::from_str(&request.args)
            .map_err(|e| format!("The arguments do not fit: {e}"))?;
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

fn failed(text: impl Into<String>) -> Reply {
    Reply::Done {
        text: text.into(),
        failed: true,
    }
}

/// Routes discovery to the named machine, defaulting to this node even
/// when it has not joined a mesh.
pub(super) async fn call(offers: &Offers, request: &Request) -> Reply {
    let args = match Args::parse(request) {
        Ok(args) => args,
        Err(e) => return failed(e),
    };
    let Some(name) = args.machine else {
        return answer(&offers.agents, request).await;
    };
    let Some(mesh) = offers.mesh.get() else {
        return failed("This machine is not in a mesh yet. Leave out `machine` to query here.");
    };
    let Some(target) = machines(&mesh)
        .into_iter()
        .find(|m| m.name == command_name(&name))
    else {
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
    let args = match Args::parse(request) {
        Ok(args) => args,
        Err(e) => return failed(e),
    };
    let Some(models) = agents.models(&args.harness).await else {
        let kinds = agents.kinds();
        let names: Vec<&str> = kinds.iter().map(|kind| kind.name.as_str()).collect();
        return failed(format!(
            "This machine has no {} agent. It has: {}.",
            args.harness,
            names.join(", ")
        ));
    };
    match serde_json::to_string(&HarnessInfo::new(args.harness, models)) {
        Ok(text) => Reply::Done {
            text,
            failed: false,
        },
        Err(e) => failed(format!("Could not write the harness info: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agents::{
            AgentKind,
            mcp::{Backend, Node},
        },
        link::peer_tools::{self, Caller},
        machine::provision::Provision,
    };
    use beep_proto::node::tools::Tool;
    use serde_json::{Value, json};
    use std::sync::Arc;

    fn request(args: Value) -> Request {
        Request {
            session: "parent".to_owned(),
            tool: Tool::HarnessInfo,
            args: args.to_string(),
            approved: Default::default(),
        }
    }

    fn result(reply: Reply) -> Value {
        match reply {
            Reply::Done {
                text,
                failed: false,
            } => serde_json::from_str(&text).expect("JSON info"),
            other => panic!("not harness info: {other:?}"),
        }
    }

    #[tokio::test]
    async fn invalid_requests_fail_before_querying_a_harness() {
        let dir = tempfile::tempdir().expect("a temp folder");
        let node = Node {
            offers: Offers::local(&[], Provision::new(dir.path())),
        };
        for (args, message) in [
            (json!({}), "arguments do not fit"),
            (json!({"harness": "  "}), "Give a `harness`"),
            (json!({"harness": "missing"}), "has no missing agent"),
            (
                json!({"harness": "missing", "model": "fast"}),
                "unknown field",
            ),
            (
                json!({"harness": "missing", "machine": "build-box"}),
                "not in a mesh",
            ),
        ] {
            let reply = node.call(&request(args)).await.expect("a reply");
            assert!(
                matches!(reply, Reply::Done { failed: true, ref text } if text.contains(message)),
                "{reply:?}"
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn local_and_peer_queries_discover_models_without_starting_a_coding_turn() {
        let dir = tempfile::tempdir().expect("a temp folder");
        let marker = dir.path().join("probes");
        let script = r#"
printf x >> "$PROBE_COUNT"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
read -r line
reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line
case "$line" in
*session/new*) reply '{"sessionId":"probe","models":{"availableModels":[{"modelId":"fast","name":"Fast","description":"Quick tasks"},{"modelId":"careful","name":"Careful"}],"currentModelId":"fast"}}' ;;
*) exit 1 ;;
esac
cat > /dev/null
"#;
        let kind = AgentKind {
            name: "codex".to_owned(),
            program: "sh".to_owned(),
            args: vec!["-c".to_owned(), script.to_owned()],
            env: [("PROBE_COUNT".to_owned(), marker.display().to_string())].into(),
            compact: None,
        };
        let mut offers = Offers::local(&[], Provision::new(dir.path()));
        offers.agents = Arc::new(Agents::new(vec![kind]));
        let node = Node { offers };
        let info = result(
            node.call(&request(json!({"harness": " codex "})))
                .await
                .expect("a reply"),
        );
        assert_eq!(info["harness"], "codex");
        assert_eq!(info["title"], "Codex");
        assert_eq!(info["model_selection"], "Listed");
        assert_eq!(info["default_model"], "fast");
        assert_eq!(
            info["models"][0],
            json!({"id": "fast", "name": "Fast", "description": "Quick tasks"})
        );
        assert_eq!(info["models"][1]["id"], "careful");
        assert_eq!(info["effort_levels"], json!(["low", "medium", "high"]));
        assert!(
            node.offers.agents.lock().is_empty(),
            "discovery starts no coding session"
        );

        // Peers can query a desktop even when it lends no commands or
        // agents. Its machine argument must not cause another forward.
        let mut caller = Caller::local(&node.offers);
        caller.desktop_here = true;
        caller.commands_off = Some("Commands are off".to_owned());
        let peer = result(
            peer_tools::answer(
                request(json!({"harness": "codex", "machine": "build-box"})),
                caller,
            )
            .await,
        );
        assert_eq!(peer, info);
        assert_eq!(
            std::fs::read_to_string(marker).expect("probe count"),
            "x",
            "the second query uses the model cache"
        );
    }

    #[test]
    fn unlisted_and_fixed_models_keep_their_selection_behavior() {
        for pick in [ModelPick::Named, ModelPick::Fixed] {
            let info = HarnessInfo::new(
                "custom-agent".to_owned(),
                ModelList {
                    pick,
                    ..ModelList::default()
                },
            );
            assert_eq!(info.model_selection, pick);
            assert_eq!(info.title, "custom-agent");
            assert!(info.default_model.is_none());
            assert!(info.models.is_empty());
            assert!(info.effort_levels.is_empty());
        }
    }
}
