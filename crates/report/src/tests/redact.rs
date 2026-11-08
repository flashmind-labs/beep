use super::*;

const PEM_IN: &str =
    "-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkq\nabcdef==\n-----END PRIVATE KEY-----";
const JWT: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.c2lnbmF0dXJl";

fn table() -> Vec<(String, String)> {
    let t = |a: &str, b: &str| (a.to_string(), b.to_string());
    vec![
        t("Authorization: Bearer abc.def", "Authorization: [redacted]"),
        t("sent Bearer abc123XYZ now", "sent Bearer [redacted] now"),
        t(
            "GET /x?token=s3cr3t&page=2",
            "GET /x?token=[redacted]&page=2",
        ),
        t("/cb?code=abc&sig=zz", "/cb?code=[redacted]&sig=[redacted]"),
        t("{\"api_key\":\"k-123\"}", "{\"api_key\":\"[redacted]\"}"),
        t("password = 'hunter2'", "password = '[redacted]'"),
        t("db_password: hunter2", "db_password: [redacted]"),
        t("OPENAI_API_KEY=sk-abc123", "OPENAI_API_KEY=[redacted]"),
        t("Cookie: session=abc", "Cookie: [redacted]"),
        t("pat ghp_abcdefghijklmnopqrstuvwx end", "pat [redacted] end"),
        t("github_pat_abcdefghijklmnopqrstuvwx", "[redacted]"),
        t("glpat-abcdefghijklmnopqrstu", "[redacted]"),
        t("key sk-ant-api03-abcdefghijklmnopqrstu", "key [redacted]"),
        t("slack xoxb-1234567890-abcdef", "slack [redacted]"),
        t("aws AKIAABCDEFGHIJKLMNOP", "aws [redacted]"),
        t("g AIzaabcdefghijklmnopqrstuvwxyz012345678", "g [redacted]"),
        t(&format!("jwt {JWT}"), "jwt [redacted]"),
        t(PEM_IN, "[redacted]"),
        t("join beepjoin2abcdefghijklmnop", "join [redacted]"),
        t("join beepjoinabcdefghijklmnop", "join [redacted]"),
        t(
            "Creds { token: Some(\"opaque-value-123\") }",
            "Creds { token: Some(\"[redacted]\") }",
        ),
        t("token: Token(\"opaque\")", "token: Token(\"[redacted]\")"),
        t("token=Some(\"opaque\")", "token=Some(\"[redacted]\")"),
        t(
            "authorization: Bearer abc, host=build-box path=/path/to/work",
            "authorization: [redacted], host=build-box path=/path/to/work",
        ),
        t(
            "-----BEGIN RSA PRIVATE KEY-----\nMIIEvQ\ncut off",
            "[redacted]",
        ),
        t("Cookie: a=1; b=2", "Cookie: [redacted]"),
        t("token=\"ab\\\"cd\"", "token=\"[redacted]\""),
        t("X-Api-Key: abcd1234", "X-Api-Key: [redacted]"),
        t("privateKey=abcd1234", "privateKey=[redacted]"),
        t(
            "https://user:pass1234@host/x",
            "https://user:[redacted]@host/x",
        ),
        t("run --token abcd1234 --x", "run --token [redacted] --x"),
        t("--password=hunter22", "--password=[redacted]"),
        t("--api-key abcd1234", "--api-key [redacted]"),
        t("sk-abcdefghijklmnopqrstuvwx", "[redacted]"),
        t(
            "Some(Token(\"opaque-value-123\"))",
            "Some(Token(\"opaque-value-123\"))",
        ),
        t(
            "token: Some(Token(\"opaque-value-123\"))",
            "token: Some(Token(\"[redacted]\"))",
        ),
        t(
            "Config { api_key: Some(Secret(\"sk_live_value\")), n: 1 }",
            "Config { api_key: Some(Secret(\"[redacted]\")), n: 1 }",
        ),
        t("token: Some(abc123xyz)", "token: Some([redacted])"),
        t("token=(abcdefgh123)", "token=([redacted])"),
        t("password=12345678", "password=[redacted]"),
        t("token=1234567890123456789", "token=[redacted]"),
        t("pin_secret=000111", "pin_secret=[redacted]"),
        t(
            "access_tokens=abc123secretvalue",
            "access_tokens=[redacted]",
        ),
        t("GITHUB_TOKENS=abcdef123456789", "GITHUB_TOKENS=[redacted]"),
        t(
            "authorization: Bearer\nnext_line_value keep",
            "authorization: [redacted]\nnext_line_value keep",
        ),
        t("COMPOSIO_KEY=ak_abc123def456", "COMPOSIO_KEY=[redacted]"),
        t(
            "export STRIPE_KEY=rk_live_abc123def456",
            "export STRIPE_KEY=[redacted]",
        ),
        t(
            "website_key = \"wk_live_abc123def456\"",
            "website_key = \"[redacted]\"",
        ),
        t(
            "R2_ACCESS_KEY_ID=AKIAEXAMPLE1234",
            "R2_ACCESS_KEY_ID=[redacted]",
        ),
        t("ssh_key=/path/to/key", "ssh_key=/path/to/key"),
        t(
            "R2_SECRET_ACCESS_KEY=/aB3dE5fG7hJ9kL1mN3pQ5rS7",
            "R2_SECRET_ACCESS_KEY=[redacted]",
        ),
        t("api_key=/aB3dE5fG7hJ9kL1", "api_key=[redacted]"),
        t("STRIPE_KEY=/abc123+def456", "STRIPE_KEY=[redacted]"),
        t("STRIPE_KEY=/abc/123+def456", "STRIPE_KEY=[redacted]"),
        t(
            "--token-file /path/to/work/tok --path-x",
            "--token-file /path/to/work/tok --path-x",
        ),
    ]
}

#[test]
fn redact_removes_each_secret_shape() {
    for (input, expected) in table() {
        assert_eq!(redact(&input).expect("rules build"), expected, "{input}");
    }
}

#[test]
fn redact_keeps_ids_paths_and_names() {
    let line = format!(
        "peer {} at /path/to/work/repo on build-box ({})",
        "0123456789abcdef".repeat(4),
        "ybndrfg8ejkmcpqxot1uwisza345h769"
    );
    assert_eq!(redact(&line).expect("rules build"), line);
}

#[test]
fn redact_keeps_ordinary_text() {
    for line in [
        "input_tokens=500 output_tokens=700 cache_read_tokens: 12",
        "token_count=42 max_tokens: 8192",
        "no token: falling back",
        "the password: is wrong",
        "credential_store: loaded 3 credentials",
        "prompt: \"fix the password: field in the form\"",
        "/path/to/work/sk-learn-thing/file.rs",
        "task-sk-abcdefgh",
        "the bearer of news",
        "token: None",
    ] {
        assert_eq!(redact(line).expect("rules build"), line);
    }
}

#[test]
fn redact_is_idempotent() {
    for (input, _) in table() {
        let once = redact(&input).expect("rules build");
        assert_eq!(redact(&once).expect("rules build"), once, "{input}");
    }
}
