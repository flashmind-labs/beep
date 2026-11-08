use super::*;

#[test]
fn reads_fenced_answers() {
    let answer = "```json\n{\"changes\":[{\"op\":\"create\",\"name\":\"Book padel\",\"description\":\"Booking a court\",\"body\":\"1. Ask\"},{\"op\":\"update\",\"name\":\"Plan trips\",\"body\":\"2. Go\"}]}\n```";
    assert_eq!(
        parse_ops(answer).unwrap(),
        [
            SkillOp::Create {
                name: "Book padel".into(),
                description: "Booking a court".into(),
                body: "1. Ask".into(),
            },
            SkillOp::Update {
                name: "Plan trips".into(),
                description: None,
                body: "2. Go".into(),
            },
        ]
    );
    assert!(parse_ops("{\"changes\":[]}").unwrap().is_empty());
    assert!(parse_ops("nothing to learn").is_err());
    assert!(parse_ops("} nothing to learn {").is_err());
}
