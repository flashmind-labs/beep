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
        inherits_compaction: false,
        quantizations: None,
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
    let not_openrouter = FlshSettings {
        models: vec![FlshModelRule {
            quantizations: Some(Vec::new()),
            ..rule("anthropic:claude-model", None)
        }],
        ..FlshSettings::default()
    };
    assert!(matches!(
        not_openrouter.check(),
        Err(FlshSettingsError::NotOpenRouter(_))
    ));
    let twice_per_model = FlshSettings {
        models: vec![FlshModelRule {
            quantizations: Some(vec![FlshQuantization::Fp4, FlshQuantization::Fp4]),
            ..rule("openrouter:vendor/model", None)
        }],
        ..FlshSettings::default()
    };
    assert_eq!(
        twice_per_model.check(),
        Err(FlshSettingsError::DuplicateQuantization("fp4"))
    );
}

/// A rule a released desktop wrote, with only a model and its compaction,
/// reads as its own compaction and the quantizations for all models.
#[test]
fn an_older_rule_reads_with_its_own_compaction() {
    #[derive(serde::Serialize)]
    struct Older {
        model: FlshModel,
        autocompact: FlshAutoCompact,
    }
    let older = Older {
        model: model("openrouter:vendor/model"),
        autocompact: FlshAutoCompact {
            enabled: false,
            tokens: None,
        },
    };
    let bytes = rmp_serde::to_vec(&older).expect("encodes");
    let read: FlshModelRule = crate::lenient::from_slice(&bytes).expect("reads");
    assert_eq!(
        read,
        FlshModelRule {
            autocompact: older.autocompact,
            ..rule("openrouter:vendor/model", None)
        }
    );
}

#[test]
fn released_settings_have_no_explicit_stream_timeout() {
    let older = (
        Some(true),
        Vec::<FlshQuantization>::new(),
        Some(FlshSort::Price),
        Some(false),
        None::<FlshAutoCompact>,
        Vec::<FlshModelRule>::new(),
    );
    let bytes = rmp_serde::to_vec(&older).expect("encodes");
    let read: FlshSettings = crate::lenient::from_slice(&bytes).expect("reads");
    assert_eq!(read.stream_idle_timeout_secs, None);
    assert_eq!(read.sort, Some(FlshSort::Price));
}
