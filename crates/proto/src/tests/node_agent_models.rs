use super::*;

#[test]
fn old_model_entries_default_context_limits() {
    let old = rmp_serde::to_vec(&("model", "Model", Option::<String>::None)).unwrap();
    let decoded: ModelOption = crate::lenient::from_slice(&old).unwrap();
    assert_eq!(decoded.context_window, None);
    assert_eq!(decoded.max_context_window, None);
    assert_eq!(decoded.provider, None);
}

#[test]
fn model_ids_are_short_and_plain() {
    assert!(valid_model("openrouter:anthropic/claude-sonnet-4"));
    assert!(valid_model("gpt-5.1-codex[high]"));
    assert!(!valid_model(""));
    assert!(!valid_model("a\nb"));
    assert!(!valid_model(&"m".repeat(MAX_MODEL + 1)));
}

#[test]
fn fast_configuration_has_its_own_wire_operation() {
    use crate::agents::AgentSteer;
    let old = rmp_serde::to_vec(&(4u8, Option::<String>::None, Option::<Effort>::None)).unwrap();
    assert_eq!(
        rmp_serde::from_slice::<AgentSteer>(&old).unwrap(),
        AgentSteer::Configure {
            model: None,
            effort: None
        }
    );
    let fast = AgentSteer::ConfigureFast {
        model: None,
        effort: None,
        fast: false,
    };
    let encoded = rmp_serde::to_vec(&fast).unwrap();
    assert_eq!(rmp_serde::from_slice::<AgentSteer>(&encoded).unwrap(), fast);
}
