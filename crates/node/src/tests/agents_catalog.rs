use super::*;
use crate::agents::Parts;
use crate::agents::{
    Agents,
    adapters::installs::Installs,
    harnesses::{Latest, Settings},
    models::ModelCache,
};
use beep_proto::node::agent;
use std::{collections::BTreeMap, time::Duration};

fn kind(name: &str) -> AgentKind {
    AgentKind {
        name: name.to_owned(),
        program: name.to_owned(),
        args: Vec::new(),
        env: BTreeMap::new(),
        compact: None,
        compact_models: Vec::new(),
        default_model: None,
        bridge: None,
    }
}

/// Every field is a value of a type below the catalog: building it takes the
/// agents and nothing of `Agents`, and a new field fails here until its type
/// is named.
#[test]
fn a_catalog_is_built_from_values() {
    let Catalog {
        kinds,
        adapters,
        models,
        settings,
        instructions,
        latest,
        settled,
        mcp,
        skills_dir,
    } = Catalog::new(vec![kind("claude")], None, None);
    let _: &watch::Sender<Arc<Vec<AgentKind>>> = &kinds;
    let _: &Installs = &adapters;
    let _: &ModelCache = &models;
    let _: &Settings = &settings;
    let _: &Mutex<Option<String>> = &instructions;
    let _: &Latest = &latest;
    let _: &watch::Sender<bool> = &settled;
    let _: &Option<Arc<Launch>> = &mcp;
    let _: &Option<PathBuf> = &skills_dir;

    assert_eq!(watched::read_sent(&kinds, |kinds| kinds.len()), 1);
}

#[test]
fn the_agents_a_catalog_holds_are_found_by_name_and_protocol() {
    let agents = Agents::new(Parts {
        kinds: vec![kind("claude"), kind("codex")],
        ..Parts::default()
    });

    assert_eq!(
        agents.catalog.named("codex").map(|kind| kind.name),
        Some("codex".to_owned())
    );
    assert!(agents.catalog.named("pi").is_none());
    assert_eq!(
        agent::agent_of(&agent::protocol("claude"))
            .and_then(|name| agents.catalog.named(name))
            .map(|kind| kind.name),
        Some("claude".to_owned())
    );
}

#[test]
fn a_new_detection_wakes_the_followers_once_and_only_when_it_differs() {
    let agents = Agents::new(Parts {
        kinds: vec![kind("claude")],
        ..Parts::default()
    });
    let mut changed = agents.catalog.kinds_changed();
    watched::read_and_update(&mut changed, |_| ());

    agents.catalog.set_kinds(vec![kind("claude")]);
    assert!(!changed.has_changed().unwrap());

    agents.catalog.set_kinds(vec![kind("claude"), kind("pi")]);
    assert!(changed.has_changed().unwrap());
    assert_eq!(agents.catalog.kinds().len(), 2);
}

/// The adapter writes the `session/new` request it got into the file named
/// by its first argument, then answers like an idle agent.
#[cfg(unix)]
const RECORDING_ADAPTER: &str = r#"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
read -r line
reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line
printf '%s\n' "$line" > "$1"
reply '{"sessionId":"s"}'
while read -r line; do reply '{}'; done
"#;

/// What the agent was told of MCP servers when a session started on a node
/// whose catalog holds `launch`.
#[cfg(unix)]
async fn mcp_servers_told(launch: Option<Arc<Launch>>) -> serde_json::Value {
    use beep_proto::node::agent::Status;

    let dir = tempfile::tempdir().expect("a temp folder");
    let log = dir.path().join("new.json");
    let mut kind = kind("claude");
    kind.program = "sh".into();
    kind.args = vec![
        "-c".into(),
        RECORDING_ADAPTER.into(),
        "adapter".into(),
        log.display().to_string(),
    ];
    let agents = Agents::new(Parts {
        kinds: vec![kind.clone()],
        launch,
        ..Parts::default()
    });
    let session = agents
        .starter
        .start(&kind, dir.path().to_str(), crate::agents::Begin::default())
        .await
        .expect("a session starts");
    let mut live = session.live.clone();
    tokio::time::timeout(
        Duration::from_secs(60),
        watched::wait_for(&mut live, |live| live.status() != Status::Starting),
    )
    .await
    .expect("the session left starting")
    .expect("the session lives");
    let told: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&log).expect("the adapter was asked"))
            .expect("json");

    told["params"]["mcpServers"].clone()
}

#[cfg(unix)]
#[tokio::test]
async fn a_session_gets_the_mcp_server_only_when_the_catalog_holds_its_launch() {
    let none = mcp_servers_told(None).await;
    assert_eq!(none, serde_json::json!([]));

    let dir = tempfile::tempdir().expect("a temp folder");
    let launch = Arc::new(Launch::new(
        PathBuf::from("beep-node"),
        dir.path().to_owned(),
    ));
    let some = mcp_servers_told(Some(launch)).await;
    assert_eq!(some.as_array().map(Vec::len), Some(1));
    assert_eq!(some[0]["name"], crate::agents::calls::SERVER_NAME);
}

#[test]
fn a_catalog_without_values_lists_no_skills_and_no_server() {
    let catalog = Catalog::new(vec![kind("claude")], None, None);

    assert!(catalog.mcp.is_none());
    assert!(catalog.skills_dir.is_none());
}

#[test]
fn auto_picks_what_the_desktop_picks_from_the_catalog_alone() {
    let kinds = vec![kind("pi"), kind("claude")];
    let host = beep_proto::AgentHost {
        machine_id: String::new(),
        machine_name: String::new(),
        agents: kinds.iter().map(|kind| kind.name.clone()).collect(),
        workspace: None,
        home: None,
        capabilities: Vec::new(),
    };
    let picked = beep_mesh::machine::pick_agent(&host, None).expect("an agent is picked");

    let catalog = Catalog::new(kinds, None, None);

    assert_eq!(catalog.auto_harness().map(|kind| kind.name), Some(picked));
}
