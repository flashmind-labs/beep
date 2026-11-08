use super::*;

const LINE: &str = r#"{"type":"assistant","sessionId":"0f3c2a9e-1111-4222-8333-944455556666","cwd":"/srv/project","timestamp":"2026-11-02T10:15:00.000Z","requestId":"req_1","message":{"id":"msg_1","model":"claude-opus-5-5","usage":{"input_tokens":2,"output_tokens":300,"cache_read_input_tokens":30000,"cache_creation_input_tokens":2000,"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":2000}}}}"#;

#[test]
fn an_assistant_line_gives_its_request() {
    assert!(wanted(LINE));
    let Some(Line::Request(request)) = line(LINE) else {
        panic!("a request");
    };
    assert_eq!(request.model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(request.tokens.cache_write_long, 2000);
    assert_eq!(request.tokens.cache_write, 0);
    assert_eq!(request.at, 1_793_614_500);
    assert!(request.key.is_some());
    assert_eq!(request.cwd.as_deref(), Some("/srv/project"));
    assert_eq!(request.billing, Billing::Unknown);
    let bedrock = LINE.replace("msg_1", "msg_bdrk_1");
    let Some(Line::Request(request)) = line(&bedrock) else {
        panic!("a request");
    };
    assert_eq!(request.billing, Billing::Metered);
}
