use regex::{Captures, Regex};
use std::sync::LazyLock;

const MASK: &str = "[redacted]";

const SECRET_KEY: &str = r"[A-Za-z0-9_.-]*(?:token|secret|password|passwd|api[_-]?key|private[_-]?key|credential|cookie|[_-]key(?:[_-]?id)?\b|access[_-]?key)[A-Za-z0-9_.-]*";
const SEP: &str = r#"["']?\s*[:=]\s*"#;
const WRAPPER: &str = r"(?:[A-Za-z_][A-Za-z0-9_]*\()*";
const OPEN: &str = r"(?:[A-Za-z_][A-Za-z0-9_]*)?\(";
const QUOTED: &str = r#""(?:\\.|[^"\\\r\n])*""#;
const QUOTED_SINGLE: &str = r"'(?:\\.|[^'\\\r\n])*'";
const BARE: &str = r#"[^\s,;&"'}\[(][^\s,;&"'}()]*"#;

#[derive(Debug, thiserror::Error)]
pub enum RedactError {
    #[error("a redaction rule did not build: {0}")]
    Rule(#[from] regex::Error),
}

/// When a match is ordinary text and stays as it is.
#[derive(Clone, Copy)]
enum Skip {
    Never,
    /// A numeric counter, or a value that is a null, a plain word or a call.
    CounterOrOrdinary,
    /// A flag followed by another flag, or by an ordinary value.
    Flag,
}

struct Rule {
    pattern: String,
    replacement: String,
    skip: Skip,
}

fn rule(pattern: impl Into<String>, replacement: impl Into<String>, skip: Skip) -> Rule {
    Rule {
        pattern: pattern.into(),
        replacement: replacement.into(),
        skip,
    }
}

fn whole(pattern: &str) -> Rule {
    rule(pattern, MASK, Skip::Never)
}

fn keep(pattern: impl Into<String>) -> Rule {
    rule(pattern, format!("${{1}}{MASK}"), Skip::Never)
}

fn rules() -> Vec<Rule> {
    vec![
        whole(r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----(?:.*?-----END [A-Z ]*PRIVATE KEY-----|.*)"),
        keep(format!(
            r#"(?i)(\bauthorization["']?\s*[:=]\s*)(?:{QUOTED}|{QUOTED_SINGLE}|(?:\w+[ \t]+)?[^\s,;"']+)"#
        )),
        keep(r"(?i)\b(bearer\s+)[A-Za-z0-9._~+/=-]{8,}"),
        keep(r#"(?i)(\b(?:set-)?cookie\s*[:=]\s*)[A-Za-z0-9_.-]+=[^\r\n"']*"#),
        keep(
            r#"(?i)([?&](?:token|access_token|key|secret|password|sig|signature|code)=)[^&\s"']*"#,
        ),
        rule(
            r"(?i)(\b[a-z][a-z0-9+.-]*://[^\s:/@]+:)[^\s@/]+(@)",
            format!("${{1}}{MASK}${{2}}"),
            Skip::Never,
        ),
        rule(
            format!(r"(?i)((?P<key>{SECRET_KEY}){SEP}{WRAPPER}){QUOTED}"),
            format!("${{1}}\"{MASK}\""),
            Skip::Never,
        ),
        rule(
            format!(r"(?i)((?P<key>{SECRET_KEY}){SEP}{WRAPPER}){QUOTED_SINGLE}"),
            format!("${{1}}'{MASK}'"),
            Skip::Never,
        ),
        rule(
            format!(
                r#"(?i)((?P<key>{SECRET_KEY}){SEP}(?:{OPEN})+)(?P<val>[^\s,;&"'}}\[()][^\s,;&"'}}()]*)"#
            ),
            format!("${{1}}{MASK}"),
            Skip::CounterOrOrdinary,
        ),
        rule(
            format!(r"(?i)((?P<key>{SECRET_KEY}){SEP})(?P<val>{BARE})"),
            format!("${{1}}{MASK}"),
            Skip::CounterOrOrdinary,
        ),
        rule(
            r"(?i)(--(?P<key>[a-z-]*(?:token|password|passwd|secret|api-key|private-key|credential)[a-z-]*)(?:\s+|=))(?P<val>\S+)",
            format!("${{1}}{MASK}"),
            Skip::Flag,
        ),
        whole(r"\b(?:ghp|gho|ghs|ghu)_[A-Za-z0-9]{20,}"),
        whole(r"\bgithub_pat_[A-Za-z0-9_]{20,}"),
        whole(r"\bglpat-[A-Za-z0-9_-]{20,}"),
        keep(r"(?m)(^|[^A-Za-z0-9/_-])sk-[A-Za-z0-9_-]{20,}"),
        whole(r"\bxox[abpr]-[A-Za-z0-9-]{10,}"),
        whole(r"\bAKIA[0-9A-Z]{16}\b"),
        whole(r"\bAIza[0-9A-Za-z_-]{35}"),
        whole(r"\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+"),
        whole(r"\bbeepjoin[a-z2-7]{10,}"),
    ]
}

struct Compiled {
    regex: Regex,
    replacement: String,
    skip: Skip,
}

impl Compiled {
    fn skips(&self, caps: &Captures, haystack: &str) -> bool {
        let key = caps
            .name("key")
            .map(|k| k.as_str().to_ascii_lowercase())
            .unwrap_or_default();
        let counter = key.ends_with("tokens") || key.ends_with("_count") || key.ends_with("-count");
        let Some(val) = caps.name("val") else {
            return false;
        };
        let val = val.as_str();
        let numeric = val.chars().all(|c| c.is_ascii_digit() || c == '.');
        match self.skip {
            Skip::Never => false,
            Skip::CounterOrOrdinary => {
                (counter && numeric)
                    || ordinary_word(val)
                    || key_path(&key, val)
                    || haystack[caps.name("val").map_or(0, |v| v.end())..].starts_with('(')
            }
            Skip::Flag => {
                (counter && numeric)
                    || val.starts_with('-')
                    || ordinary_word(val)
                    || key.ends_with("-file")
                    || key.ends_with("-path")
            }
        }
    }
}

/// A key file's path, as in `ssh_key=/path/to/key`.
fn key_path(key: &str, val: &str) -> bool {
    let key = key.trim_end_matches("id").trim_end_matches(['_', '-']);
    key.ends_with("key") && val.starts_with('/')
}

/// A null or boolean, or a short plain word: not a secret.
fn ordinary_word(val: &str) -> bool {
    let lower = val.to_ascii_lowercase();
    matches!(lower.as_str(), "null" | "none" | "nil" | "true" | "false")
        || (val.len() < 8 && val.chars().all(|c| c.is_ascii_alphabetic()))
}

static COMPILED: LazyLock<Result<Vec<Compiled>, regex::Error>> = LazyLock::new(|| {
    rules()
        .into_iter()
        .map(|r| {
            Ok(Compiled {
                regex: Regex::new(&r.pattern)?,
                replacement: r.replacement,
                skip: r.skip,
            })
        })
        .collect()
});

/// Replaces secrets in `text` with `[redacted]`, keeping the key name of `key=value` shapes.
/// Fails closed: if a rule did not build, nothing is returned.
pub fn redact(text: &str) -> Result<String, RedactError> {
    let compiled = COMPILED
        .as_ref()
        .map_err(|e| RedactError::Rule(e.clone()))?;
    let mut out = text.to_string();
    for rule in compiled {
        let next = rule
            .regex
            .replace_all(&out, |caps: &Captures| {
                if rule.skips(caps, &out) {
                    return caps[0].to_string();
                }
                let mut replaced = String::new();
                caps.expand(&rule.replacement, &mut replaced);
                replaced
            })
            .into_owned();
        out = next;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
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
}
