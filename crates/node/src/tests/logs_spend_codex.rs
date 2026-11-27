use super::*;

#[test]
fn a_log_gives_its_session_model_and_counts() {
    let meta = r#"{"timestamp":"2026-11-02T10:00:00.000Z","type":"session_meta","payload":{"id":"0199a1b2-3c4d-7e5f-8a9b-0c1d2e3f4a5b","cwd":"/srv/project","source":"cli","model_provider":"openai"}}"#;
    assert!(wanted(meta));
    assert!(matches!(
        line(meta),
        Some(Line::Session { id, cwd, provider })
            if id.as_ref().starts_with("0199") && cwd == "/srv/project" && provider.as_deref() == Some("openai")
    ));
    let turn = r#"{"timestamp":"2026-11-02T10:00:01.000Z","type":"turn_context","payload":{"cwd":"/srv/project","model":"gpt-6.1-sol"}}"#;
    assert!(matches!(line(turn), Some(Line::Model(model)) if model == "gpt-6.1-sol"));
    let count = r#"{"timestamp":"2026-11-02T10:15:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":28949,"cached_input_tokens":28032,"output_tokens":99,"total_tokens":29048},"last_token_usage":{"input_tokens":28949,"cached_input_tokens":28032,"output_tokens":99,"total_tokens":29048}}}}"#;
    let Some(Line::Request(request)) = line(count) else {
        panic!("a request");
    };
    assert_eq!(request.tokens.input, 917);
    assert_eq!(request.tokens.cache_read, 28032);
    assert_eq!(request.tokens.output, 99);
    assert_eq!(request.total, Some(29048));
    assert_eq!(request.at, 1_793_614_500);
    assert_eq!(request.billing, Billing::Unknown);
    let planned = count.replace(
        r#""info":{"#,
        r#""rate_limits":{"plan_type":"pro"},"info":{"#,
    );
    let Some(Line::Request(request)) = line(&planned) else {
        panic!("a request");
    };
    assert_eq!(request.billing, Billing::Subscription);
}
