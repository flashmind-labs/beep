use super::*;

#[test]
fn a_plan_reads_its_rounds_scouts() {
    let model = RoleModel {
        model: "openai:test".parse().expect("a model id"),
        thinking: ReasoningLevel::Medium,
    };
    let scout = |question: &str, round| Scout {
        question: question.to_owned(),
        model: model.clone(),
        round,
    };
    let plan = Plan {
        scouts: vec![scout("a", 1), scout("b", 1), scout("c", 2)],
        critic: model.clone(),
        writer: model.clone(),
    };
    assert_eq!(plan.round(), 2);
    let second: Vec<Member> = plan.scouts_of(2).map(|(member, _)| member).collect();
    assert_eq!(second, [Member::Scout(3)]);
    let old: Plan = serde_json::from_value(serde_json::json!({
        "scouts": [{ "question": "a", "model": model }],
        "critic": model,
        "writer": model,
    }))
    .expect("a plan from before rounds");
    assert_eq!(old.round(), 1);
}
