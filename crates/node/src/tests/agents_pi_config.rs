use super::*;
use beep_proto::{FlshAutoCompact, FlshModelRule, FlshSort};
use serde_json::json;

fn read(dir: &Path) -> Value {
    let text = fs::read_to_string(dir.join(MODELS)).expect("models.json reads");
    serde_json::from_str(&text).expect("models.json is JSON")
}

fn rule(name: &str, quantizations: Option<Vec<FlshQuantization>>) -> FlshModelRule {
    FlshModelRule {
        model: name.parse().expect("a valid model"),
        autocompact: FlshAutoCompact {
            enabled: true,
            tokens: None,
        },
        inherits_compaction: true,
        quantizations,
    }
}

fn full() -> FlshSettings {
    FlshSettings {
        zdr: Some(true),
        quantizations: vec![FlshQuantization::Fp8, FlshQuantization::Bf16],
        sort: Some(FlshSort::Throughput),
        allow_fallbacks: Some(false),
        models: vec![
            rule("openrouter:vendor/any", Some(Vec::new())),
            rule("openrouter:vendor/small", Some(vec![FlshQuantization::Fp4])),
            rule("openrouter:vendor/compact-only", None),
        ],
        ..FlshSettings::default()
    }
}

#[test]
fn pi_reads_the_routing_where_it_looks() {
    let dir = tempfile::tempdir().unwrap();
    apply(dir.path(), &full()).unwrap();
    let all: Vec<&str> = FlshQuantization::VARIANTS
        .iter()
        .map(|q| q.into())
        .collect();
    assert_eq!(
        read(dir.path()),
        json!({"providers": {"openrouter": {
            "compat": {"openRouterRouting": {
                "zdr": true,
                "quantizations": ["fp8", "bf16"],
                "sort": "throughput",
                "allow_fallbacks": false,
            }},
            "modelOverrides": {
                "vendor/any": {"compat": {"openRouterRouting": {"quantizations": all}}},
                "vendor/small": {"compat": {"openRouterRouting": {"quantizations": ["fp4"]}}},
            },
        }}})
    );
}

#[test]
fn the_users_own_keys_stay_and_cleared_ones_go() {
    let dir = tempfile::tempdir().unwrap();
    let mine = json!({"providers": {
        "openrouter": {
            "compat": {"openRouterRouting": {"data_collection": "deny", "sort": "price"}},
            "modelOverrides": {"vendor/mine": {"name": "Mine"}},
        },
        "example": {"baseUrl": "https://provider.example/v1"},
    }});
    fs::write(dir.path().join(MODELS), mine.to_string()).unwrap();
    apply(dir.path(), &full()).unwrap();
    let written = read(dir.path());
    assert_eq!(
        written["providers"]["example"],
        mine["providers"]["example"]
    );
    assert_eq!(
        written["providers"]["openrouter"]["modelOverrides"]["vendor/mine"],
        json!({"name": "Mine"})
    );

    apply(dir.path(), &FlshSettings::default()).unwrap();
    // The user's sort was replaced by Beep's, so clearing Beep's drops it.
    assert_eq!(
        read(dir.path()),
        json!({"providers": {
            "openrouter": {
                "compat": {"openRouterRouting": {"data_collection": "deny"}},
                "modelOverrides": {"vendor/mine": {"name": "Mine"}},
            },
            "example": {"baseUrl": "https://provider.example/v1"},
        }})
    );
}

#[test]
fn clearing_everything_leaves_no_empty_provider() {
    let dir = tempfile::tempdir().unwrap();
    apply(dir.path(), &full()).unwrap();
    apply(dir.path(), &FlshSettings::default()).unwrap();
    assert_eq!(read(dir.path()), json!({}));
}

#[test]
fn no_routing_makes_no_file_and_the_same_routing_no_write() {
    let dir = tempfile::tempdir().unwrap();
    apply(dir.path(), &FlshSettings::default()).unwrap();
    assert!(!dir.path().join(MODELS).exists());

    apply(dir.path(), &full()).unwrap();
    let path = dir.path().join(MODELS);
    let first = fs::metadata(&path).unwrap().modified().unwrap();
    let text = fs::read_to_string(&path).unwrap();
    apply(dir.path(), &full()).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), first);
}

#[test]
fn a_file_with_comments_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(MODELS);
    let text = "{\n  // mine\n  \"providers\": {}\n}\n";
    fs::write(&path, text).unwrap();
    let error = apply(dir.path(), &full()).unwrap_err();
    assert!(format!("{error:#}").contains("not plain JSON"), "{error:#}");
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
    assert!(!dir.path().join(OWNED).exists());
}
