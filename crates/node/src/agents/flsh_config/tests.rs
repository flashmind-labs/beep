use super::*;
use beep_proto::{FlshModelRule, FlshQuantization, FlshSort};
use flashmind_types::{Model, ProviderPreferences, ProviderSort, Quantization};
use std::collections::HashMap;

/// flsh's config as flsh reads it: the parts Beep writes, with flsh's own
/// types where they are public and its serde attributes copied where not
/// (flashmind-cli's `Config` and `AutoCompactSettings`).
#[derive(Debug, Deserialize)]
struct FlshConfig {
    #[serde(default)]
    openrouter: OpenRouterConfig,
    autocompact: Option<Rule>,
    #[serde(default)]
    auto_compact_models: HashMap<Model, Rule>,
}

#[derive(Debug, Default, Deserialize)]
struct OpenRouterConfig {
    routing: Option<ProviderPreferences>,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(default)]
struct Rule {
    #[serde(rename = "enable", alias = "enabled")]
    enabled: bool,
    tokens: Option<u32>,
}

impl Default for Rule {
    fn default() -> Self {
        Self {
            enabled: true,
            tokens: None,
        }
    }
}

fn read(dir: &Path) -> String {
    fs::read_to_string(dir.join(CONFIG)).expect("the config reads")
}

fn as_flsh(dir: &Path) -> FlshConfig {
    toml::from_str(&read(dir)).expect("flsh reads the config")
}

fn model(name: &str) -> Model {
    name.parse().expect("a valid model")
}

fn rule(name: &str, enabled: bool, tokens: Option<u32>) -> FlshModelRule {
    FlshModelRule {
        model: name.parse().expect("a valid model"),
        autocompact: FlshAutoCompact { enabled, tokens },
    }
}

fn full() -> FlshSettings {
    FlshSettings {
        zdr: Some(true),
        quantizations: vec![FlshQuantization::Fp8, FlshQuantization::Bf16],
        sort: Some(FlshSort::Throughput),
        allow_fallbacks: Some(false),
        autocompact: Some(FlshAutoCompact {
            enabled: true,
            tokens: Some(300_000),
        }),
        models: vec![rule("openrouter:vendor/small", true, Some(60_000))],
    }
}

#[test]
fn flsh_reads_every_setting_beep_writes() {
    let dir = tempfile::tempdir().unwrap();
    apply(dir.path(), &full()).unwrap();
    let config = as_flsh(dir.path());
    let routing = config.openrouter.routing.expect("routing is set");
    assert_eq!(routing.zdr, Some(true));
    assert_eq!(routing.sort, Some(ProviderSort::Throughput));
    assert_eq!(routing.allow_fallbacks, Some(false));
    assert_eq!(
        routing.quantizations,
        [Quantization::Fp8, Quantization::Bf16]
    );
    assert_eq!(
        config.autocompact,
        Some(Rule {
            enabled: true,
            tokens: Some(300_000)
        })
    );
    assert_eq!(
        config.auto_compact_models[&model("openrouter:vendor/small")],
        Rule {
            enabled: true,
            tokens: Some(60_000)
        }
    );
    // Tables, not inline ones, as flsh writes them itself.
    let text = read(dir.path());
    assert!(text.contains("[openrouter.routing]"), "{text}");
    assert!(
        text.contains("[auto_compact_models.\"openrouter:vendor/small\"]"),
        "{text}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(dir.path().join(CONFIG))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

const HAND_WRITTEN: &str = r#"# flsh settings
model = "openrouter:vendor/large"

[providers.openrouter]
api_key = "placeholder-key"  # kept as is

[openrouter.routing]
order = ["example-provider"]
data_collection = "deny"

[autocompact]
enabled = false

[auto_compact_models."openrouter:vendor/large"]
enabled = false
tokens = 90000
"#;

#[test]
fn the_users_own_keys_and_comments_stay() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(CONFIG), HAND_WRITTEN).unwrap();
    apply(dir.path(), &full()).unwrap();
    let text = read(dir.path());
    for kept in [
        "# flsh settings\nmodel = \"openrouter:vendor/large\"\n",
        "[providers.openrouter]\napi_key = \"placeholder-key\"  # kept as is\n",
        "order = [\"example-provider\"]\ndata_collection = \"deny\"\n",
    ] {
        assert!(text.contains(kept), "{kept:?} went:\n{text}");
    }
    let config = as_flsh(dir.path());
    let routing = config.openrouter.routing.unwrap();
    assert_eq!(routing.order, ["example-provider"]);
    assert_eq!(routing.data_collection.as_deref(), Some("deny"));
    assert_eq!(routing.zdr, Some(true));
    // `enabled` is another name for `enable` there: both would not read.
    assert!(!text.contains("[autocompact]\nenabled"), "{text}");
    assert_eq!(config.autocompact.unwrap().tokens, Some(300_000));
}

#[test]
fn clearing_a_setting_drops_only_what_beep_set() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(CONFIG), HAND_WRITTEN).unwrap();
    let mut settings = FlshSettings {
        sort: Some(FlshSort::Latency),
        models: vec![rule("openrouter:vendor/small", false, None)],
        ..FlshSettings::default()
    };
    apply(dir.path(), &settings).unwrap();
    assert!(as_flsh(dir.path()).auto_compact_models.len() == 2);

    settings.sort = None;
    settings.models.clear();
    apply(dir.path(), &settings).unwrap();
    let config = as_flsh(dir.path());
    let routing = config.openrouter.routing.unwrap();
    assert_eq!(routing.sort, None);
    // Never Beep's: they stay.
    assert_eq!(routing.data_collection.as_deref(), Some("deny"));
    assert_eq!(config.autocompact.map(|rule| rule.enabled), Some(false));
    assert_eq!(
        config.auto_compact_models.keys().collect::<Vec<_>>(),
        [&model("openrouter:vendor/large")]
    );
    // Settings that change nothing leave the file as it is.
    let text = read(dir.path());
    apply(dir.path(), &settings).unwrap();
    assert_eq!(read(dir.path()), text);
}

#[test]
fn a_model_spelled_another_way_is_replaced() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(CONFIG),
        "[auto_compact_models.\"OpenRouter:vendor/small\"]\nenable = true\n",
    )
    .unwrap();
    let settings = FlshSettings {
        models: vec![rule("openrouter:vendor/small", true, Some(50_000))],
        ..FlshSettings::default()
    };
    apply(dir.path(), &settings).unwrap();
    let text = read(dir.path());
    assert!(!text.contains("OpenRouter:"), "{text}");
    // `enable` and `enabled` together would not read.
    assert!(!text.contains("enable ="), "{text}");
    let config = as_flsh(dir.path());
    assert_eq!(config.auto_compact_models.len(), 1);
    assert_eq!(
        config.auto_compact_models[&model("openrouter:vendor/small")].tokens,
        Some(50_000)
    );
}

#[cfg(unix)]
#[test]
fn the_configs_permissions_stay() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG);
    fs::write(&path, "model = \"ollama:example\"\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    apply(dir.path(), &full()).unwrap();
    let mode = fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o640);
}

#[test]
fn a_config_that_does_not_read_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG);
    fs::write(&path, "model = \n").unwrap();
    let error = apply(dir.path(), &full()).unwrap_err();
    assert!(format!("{error:#}").contains("not valid TOML"), "{error:#}");
    assert_eq!(fs::read_to_string(&path).unwrap(), "model = \n");
    assert!(!dir.path().join(OWNED).exists());
}
