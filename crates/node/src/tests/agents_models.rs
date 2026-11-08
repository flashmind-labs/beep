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
reply '{"sessionId":"s","models":{"availableModels":[{"modelId":"sonnet","name":"Sonnet"},{"modelId":"opus","name":"Opus"}],"currentModelId":"sonnet"}}'
cat > /dev/null
"#;

#[cfg(unix)]
fn adapter(name: &str, script: &str) -> AgentKind {
    AgentKind {
        name: name.into(),
        program: "sh".into(),
        args: vec!["-c".into(), script.into(), "fake-adapter".into()],
        env: Default::default(),
        compact: None,
        compact_models: Vec::new(),
        default_model: None,
    }
}

#[cfg(unix)]
#[tokio::test]
async fn an_acp_agent_lists_the_models_its_session_offers() {
    let listing = find(adapter("claude", MODELS_ADAPTER)).await;
    assert_eq!(listing.pick, ModelPick::Listed);
    assert_eq!(listing.current.as_deref(), Some("sonnet"));
    let names: Vec<&str> = listing.models.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["Sonnet", "Opus"]);
}
