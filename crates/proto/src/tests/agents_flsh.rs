use super::*;

fn model(name: &str) -> FlshModel {
    name.parse().expect("a valid model")
}

fn rule(name: &str, tokens: Option<u32>) -> FlshModelRule {
    FlshModelRule {
        model: model(name),
        autocompact: FlshAutoCompact {
            enabled: true,
            tokens,
        },
    }
}

#[test]
fn bad_values_are_refused() {
    let twice = FlshSettings {
        quantizations: vec![FlshQuantization::Fp8, FlshQuantization::Fp8],
        ..FlshSettings::default()
    };
    assert_eq!(
        twice.check(),
        Err(FlshSettingsError::DuplicateQuantization("fp8"))
    );

    for tokens in [0, 999, MAX_COMPACT_TOKENS + 1] {
        let global = FlshSettings {
            autocompact: Some(FlshAutoCompact {
                enabled: true,
                tokens: Some(tokens),
            }),
            ..FlshSettings::default()
        };
        assert_eq!(global.check(), Err(FlshSettingsError::Tokens));
        let per_model = FlshSettings {
            models: vec![rule("openrouter:vendor/model", Some(tokens))],
            ..FlshSettings::default()
        };
        assert_eq!(per_model.check(), Err(FlshSettingsError::Tokens));
    }
    // The same model spelled two ways is still one model.
    let duplicate = FlshSettings {
        models: vec![
            rule("openrouter:vendor/model", None),
            rule("OpenRouter:vendor/model", Some(50_000)),
        ],
        ..FlshSettings::default()
    };
    assert!(matches!(
        duplicate.check(),
        Err(FlshSettingsError::DuplicateModel(_))
    ));
    let long = FlshSettings {
        models: vec![rule(&format!("openrouter:{}", "x".repeat(300)), None)],
        ..FlshSettings::default()
    };
    assert_eq!(long.check(), Err(FlshSettingsError::BadModel));
    let many = FlshSettings {
        models: (0..=MAX_MODEL_RULES)
            .map(|i| rule(&format!("openrouter:vendor/m{i}"), None))
            .collect(),
        ..FlshSettings::default()
    };
    assert_eq!(many.check(), Err(FlshSettingsError::TooManyModels));
}
