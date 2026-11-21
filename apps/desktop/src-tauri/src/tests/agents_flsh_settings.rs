use super::*;

fn view() -> FlshView {
    FlshView {
        zdr: Some(true),
        quantizations: vec!["fp8".into(), "bf16".into()],
        sort: Some("throughput".into()),
        allow_fallbacks: None,
        autocompact: Some(FlshAutoCompact {
            enabled: true,
            tokens: Some(300_000),
        }),
        models: vec![
            ModelRuleView {
                model: "openrouter:vendor/model".into(),
                inherits_compaction: false,
                enabled: false,
                tokens: None,
                quantizations: None,
            },
            ModelRuleView {
                model: "openrouter:vendor/fp4-only".into(),
                inherits_compaction: true,
                enabled: true,
                tokens: Some(300_000),
                quantizations: Some(vec!["fp4".into()]),
            },
        ],
    }
}

#[test]
fn the_page_round_trips() {
    let settings = FlshSettings::try_from(view()).expect("valid settings");
    assert_eq!(settings.sort, Some(FlshSort::Throughput));
    assert_eq!(FlshView::from(settings), view());
}

#[test]
fn bad_text_is_refused() {
    let quant = FlshView {
        quantizations: vec!["fp7".into()],
        ..view()
    };
    assert!(FlshSettings::try_from(quant).is_err());
    let sort = FlshView {
        sort: Some("fastest".into()),
        ..view()
    };
    assert!(FlshSettings::try_from(sort).is_err());
    let model = FlshView {
        models: vec![ModelRuleView {
            model: "vendor/model".into(),
            inherits_compaction: false,
            enabled: true,
            tokens: None,
            quantizations: None,
        }],
        ..view()
    };
    assert!(FlshSettings::try_from(model).is_err());
    let tokens = FlshView {
        autocompact: Some(FlshAutoCompact {
            enabled: true,
            tokens: Some(10),
        }),
        ..view()
    };
    assert!(FlshSettings::try_from(tokens).is_err());
    let rule_quant = FlshView {
        models: vec![ModelRuleView {
            quantizations: Some(vec!["fp7".into()]),
            ..view().models.remove(0)
        }],
        ..view()
    };
    assert!(FlshSettings::try_from(rule_quant).is_err());
}

#[test]
fn an_inheriting_rule_carries_the_rule_for_all_models() {
    let settings = FlshSettings::try_from(view()).expect("valid settings");
    let rule = &settings.models[1];
    assert!(rule.inherits_compaction);
    assert_eq!(Some(rule.autocompact), settings.autocompact);
}
