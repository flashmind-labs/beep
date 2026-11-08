use super::*;

fn view() -> FlshView {
    FlshView {
        zdr: Some(true),
        quantizations: vec!["fp8".into(), "bf16".into()],
        sort: Some("throughput".into()),
        allow_fallbacks: None,
        autocompact: Some(AutoCompactView {
            enabled: true,
            tokens: Some(300_000),
        }),
        models: vec![ModelRuleView {
            model: "openrouter:vendor/model".into(),
            enabled: false,
            tokens: None,
        }],
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
            enabled: true,
            tokens: None,
        }],
        ..view()
    };
    assert!(FlshSettings::try_from(model).is_err());
    let tokens = FlshView {
        autocompact: Some(AutoCompactView {
            enabled: true,
            tokens: Some(10),
        }),
        ..view()
    };
    assert!(FlshSettings::try_from(tokens).is_err());
}
