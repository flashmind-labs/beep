use super::*;

#[test]
fn models_are_found_by_the_names_agents_give() {
    assert_eq!(price("claude-opus-5-5").map(|p| p.input), Some(4.0));
    assert_eq!(
        price("claude-opus-4-1-20250805").map(|p| p.input),
        Some(15.0)
    );
    assert_eq!(price("claude-sonnet-5-5[1m]").map(|p| p.input), Some(2.0));
    assert_eq!(
        price("openrouter:anthropic/claude-sonnet-5.5").map(|p| p.input),
        Some(2.0)
    );
    assert_eq!(price("gpt-6.1-sol").map(|p| p.output), Some(10.0));
    assert_eq!(price("openai/GPT-6-Luna").map(|p| p.input), Some(0.1));
    assert_eq!(price("claude-opus-9"), None);
    assert_eq!(price("<synthetic>"), None);
}
