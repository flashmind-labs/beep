use beep_proto::SessionId;
use super::*;
use crate::{
    agents::{
        AgentKind,
        mcp::{Backend, Node},
    },
    agents::mcp::peer_tools::{self, Caller},
    machine::provision::Provision,
};
use beep_proto::node::tools::Tool;
use serde_json::{Value, json};
use std::sync::Arc;

fn request(args: Value) -> Request {
    Request {
        session: SessionId::from("parent").to_owned(),
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
        screens: Default::default(),
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
    std::fs::write(dir.path().join("models_cache.json"), r#"{"models":[{"slug":"fast","context_window":400000,"max_context_window":800000,"effective_context_window_percent":90}]}"#).expect("a model catalog");
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
        env: [
            ("PROBE_COUNT".to_owned(), marker.display().to_string()),
            ("CODEX_HOME".to_owned(), dir.path().display().to_string()),
        ]
        .into(),
        compact: None,
        compact_models: Vec::new(),
        default_model: None,
        bridge: None,
    };
    let mut offers = Offers::local(&[], Provision::new(dir.path()));
    offers.agents = Arc::new(Agents::new(vec![kind]));
    let node = Node {
        offers,
        screens: Default::default(),
    };
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
        json!({"id": "fast", "name": "Fast", "description": "Quick tasks", "context_window": 360000, "max_context_window": 720000, "provider": null, "efforts": [], "fast": null})
    );
    assert_eq!(info["models"][1]["id"], "careful");
    assert_eq!(
        info["effort_levels"],
        json!([
            "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra"
        ])
    );
    assert!(
        node.offers.agents.session_map().is_empty(),
        "discovery starts no coding session"
    );

    // Peers can query a desktop even when it lends no commands or
    // agents. Its machine argument must not cause another forward.
    let mut caller = Caller::local(&node.offers);
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
