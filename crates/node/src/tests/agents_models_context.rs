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
