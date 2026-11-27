use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn list(current: &str) -> ModelList {
    ModelList {
        pick: ModelPick::Named,
        current: Some(current.into()),
        models: Vec::new(),
    }
}

#[tokio::test]
async fn an_agent_is_asked_once_while_its_list_is_recent() {
    let cache = ModelCache::default();
    let asked = AtomicUsize::new(0);
    let find = || async {
        asked.fetch_add(1, Ordering::SeqCst);
        list("m")
    };
    assert_eq!(cache.get("claude", find()).await, list("m"));
    assert_eq!(cache.get("claude", find()).await, list("m"));
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    cache.get("codex", find()).await;
    assert_eq!(asked.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_slow_agent_does_not_hold_back_another() {
    let cache = ModelCache::default();
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    let slow = async {
        released.await.ok();
        list("slow")
    };
    let other = async {
        let fast = cache.get("codex", async { list("fast") }).await;
        release.send(()).ok();
        fast
    };
    let both = async { tokio::join!(cache.get("claude", slow), other) };
    let (claude, codex) = tokio::time::timeout(Duration::from_secs(5), both)
        .await
        .expect("codex waited for claude's probe");
    assert_eq!(claude, list("slow"));
    assert_eq!(codex, list("fast"));
}

#[test]
fn flsh_config_is_found_where_flsh_looks() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let none = |_: &str| None;
    let flashmind_home =
        |name: &str| (name == "FLASHMIND_HOME").then(|| OsString::from("/srv/flashmind"));
    assert_eq!(
        flsh_config_dir(&flashmind_home, Some(&home)),
        Some(PathBuf::from("/srv/flashmind"))
    );
    let relative = |name: &str| (name == "FLASHMIND_HOME").then(|| OsString::from("rel"));
    assert_eq!(flsh_config_dir(&relative, Some(&home)), None);

    if !cfg!(windows) {
        assert_eq!(
            flsh_config_dir(&none, Some(&home)),
            Some(home.join(".config/flashmind"))
        );
        let xdg = |name: &str| (name == "XDG_CONFIG_HOME").then(|| OsString::from("/xdg"));
        assert_eq!(
            flsh_config_dir(&xdg, Some(&home)),
            Some(PathBuf::from("/xdg/flashmind"))
        );
    }

    std::fs::create_dir_all(home.join(".flashmind")).unwrap();
    assert_eq!(
        flsh_config_dir(&none, Some(&home)),
        Some(home.join(".flashmind"))
    );
}

#[cfg(unix)]
#[tokio::test]
async fn flsh_reads_the_config_in_the_home_it_is_set_to() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("config.toml"), "model = \"set:home\"\n").unwrap();
    let kind = AgentKind {
        name: "flsh".into(),
        program: "false".into(),
        args: Vec::new(),
        env: [("FLASHMIND_HOME".into(), dir.path().display().to_string())].into(),
        compact: None,
        compact_models: Vec::new(),
        default_model: None,
        bridge: None,
    };
    assert_eq!(flsh(&kind).await.current.as_deref(), Some("set:home"));
}

/// An adapter that answers `session/new` with models and stays up.
#[cfg(unix)]
const MODELS_ADAPTER: &str = r#"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
read -r line
reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line
reply "$1"
cat > /dev/null
"#;

#[cfg(unix)]
fn adapter(name: &str, response: serde_json::Value) -> AgentKind {
    AgentKind {
        name: name.into(),
        program: "sh".into(),
        args: vec![
            "-c".into(),
            MODELS_ADAPTER.into(),
            "fake-adapter".into(),
            response.to_string(),
        ],
        env: Default::default(),
        compact: None,
        compact_models: Vec::new(),
        default_model: None,
        bridge: None,
    }
}

#[cfg(unix)]
#[tokio::test]
async fn an_acp_agent_lists_the_models_its_session_offers() {
    let response = serde_json::json!({
        "sessionId": "s",
        "models": {
            "availableModels": [
                {"modelId": "sonnet", "name": "Sonnet"},
                {"modelId": "opus", "name": "Opus"}
            ],
            "currentModelId": "sonnet"
        }
    });
    let listing = find(adapter("claude", response)).await;
    assert_eq!(listing.pick, ModelPick::Listed);
    assert_eq!(listing.current.as_deref(), Some("sonnet"));
    let names: Vec<&str> = listing.models.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["Sonnet", "Opus"]);
}

#[cfg(unix)]
#[tokio::test]
async fn pi_lists_the_provider_separately_from_the_model_name() {
    let id = "openrouter/vendor/model:batch";
    let response = serde_json::json!({
        "sessionId": "s",
        "configOptions": [{
            "id": "model",
            "name": "Model",
            "category": "model",
            "type": "select",
            "currentValue": id,
            "options": [
                {"value": id, "name": "openrouter/Vendor: Model (batch)"},
                {"value": "custom/model", "name": "Family/Model"},
                {"value": "default", "name": "Default"}
            ]
        }]
    });
    let listing = find(adapter("pi", response)).await;

    assert_eq!(listing.pick, ModelPick::Listed);
    assert_eq!(listing.current.as_deref(), Some(id));
    let models: Vec<_> = listing
        .models
        .iter()
        .map(|model| {
            (
                model.id.as_str(),
                model.name.as_str(),
                model.provider.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        models,
        [
            (id, "Vendor: Model (batch)", Some("openrouter")),
            ("custom/model", "Family/Model", Some("custom")),
            ("default", "Default", None),
        ]
    );
}

#[cfg(unix)]
#[tokio::test]
async fn v2_only_agents_offer_models_before_starting_a_session() {
    let temp = tempfile::tempdir().unwrap();
    let kind = AgentKind {
        name: "test-agent".into(),
        program: "python3".into(),
        args: vec![
            concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/fake_acp_v2.py").into(),
            temp.path().join("requests.jsonl").display().to_string(),
        ],
        env: [("BEEP_ACP_VERSION".into(), "2".into())].into(),
        compact: None,
        compact_models: Vec::new(),
        default_model: None,
        bridge: None,
    };
    let offered = probe(&kind, temp.path()).await.unwrap();
    assert_eq!(offered.current.as_deref(), Some("test"));
    assert_eq!(offered.models.len(), 2);
    assert_eq!(offered.models[1].id, "other");
}
