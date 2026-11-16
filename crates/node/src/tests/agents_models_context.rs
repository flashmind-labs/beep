use super::*;
use beep_proto::node::agent::ModelOption;

#[test]
fn catalog_limits_are_usable_and_unknown_models_stay_unknown() {
    let mut list = ModelList {
        models: ["model-large[high]", "unknown"]
            .map(|id| ModelOption {
                id: id.into(),
                ..ModelOption::default()
            })
            .into(),
        ..ModelList::default()
    };
    apply(br#"{"models":[{"slug":"model-large","context_window":400000,"max_context_window":800000,"effective_context_window_percent":90}]}"#, &mut list);
    assert_eq!(list.models[0].context_window, Some(360000));
    assert_eq!(list.models[0].max_context_window, Some(720000));
    assert_eq!(list.models[1].context_window, None);
    apply(br#"{"models":[{"slug":"model-large","context_window":18446744073709051615,"effective_context_window_percent":90}]}"#, &mut list);
    assert_eq!(list.models[0].context_window, None);
}

#[test]
fn codex_catalog_reports_thinking_and_speed_without_context_limits() {
    let mut list = ModelList {
        models: vec![ModelOption {
            id: "model-a".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    apply(br#"{"models":[{"slug":"model-a","supported_reasoning_levels":[{"effort":"none"},{"effort":"ultra"},{"effort":"future"}],"additional_speed_tiers":["fast"]}]}"#, &mut list);
    assert_eq!(list.models[0].efforts, vec![Effort::None, Effort::Ultra]);
    assert_eq!(list.models[0].fast, Some(true));
}
