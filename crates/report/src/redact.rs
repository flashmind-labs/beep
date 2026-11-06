use regex::Regex;
use std::sync::LazyLock;

const MASK: &str = "[redacted]";

const SECRET_KEY: &str = r"[A-Za-z0-9_.-]*(?:token|secret|password|passwd|api_key|apikey|private_key|credential|cookie)[A-Za-z0-9_.-]*";
const SEP: &str = r#"["']?\s*[:=]\s*"#;
const BARE: &str = r#"[^\s,;&"'}\[][^\s,;&"'}]*"#;

#[derive(Debug, thiserror::Error)]
pub enum RedactError {
    #[error("a redaction rule did not build: {0}")]
    Rule(#[from] regex::Error),
}

struct Rule {
    pattern: String,
    replacement: String,
}

fn rule(pattern: impl Into<String>, replacement: &str) -> Rule {
    Rule {
        pattern: pattern.into(),
        replacement: replacement.to_string(),
    }
}

fn whole(pattern: &str) -> Rule {
    rule(pattern, MASK)
}

fn rules() -> Vec<Rule> {
    let keep = format!("${{1}}{MASK}");
    let keep_dq = format!("${{1}}\"{MASK}\"");
    let keep_sq = format!("${{1}}'{MASK}'");
    vec![
        whole(r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----"),
        rule(
            r#"(?i)(\bauthorization["']?\s*[:=]\s*)(?:"[^"\r\n]*"|'[^'\r\n]*'|[^\r\n]+)"#,
            &keep,
        ),
        rule(r"(?i)\b(bearer\s+)[A-Za-z0-9._~+/=-]+", &keep),
        rule(
            r#"(?i)([?&](?:token|access_token|key|secret|password|sig|signature|code)=)[^&\s"']*"#,
            &keep,
        ),
        rule(
            format!(r#"(?i)((?:{SECRET_KEY}){SEP})"[^"\r\n]*""#),
            &keep_dq,
        ),
        rule(format!(r"(?i)((?:{SECRET_KEY}){SEP})'[^'\r\n]*'"), &keep_sq),
        rule(format!(r"(?i)((?:{SECRET_KEY}){SEP}){BARE}"), &keep),
        whole(r"\b(?:ghp|gho|ghs|ghu)_[A-Za-z0-9]{20,}"),
        whole(r"\bgithub_pat_[A-Za-z0-9_]{20,}"),
        whole(r"\bglpat-[A-Za-z0-9_-]{20,}"),
        whole(r"\bsk-[A-Za-z0-9_-]{6,}"),
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
}

static COMPILED: LazyLock<Result<Vec<Compiled>, regex::Error>> = LazyLock::new(|| {
    rules()
        .into_iter()
        .map(|r| {
            Ok(Compiled {
                regex: Regex::new(&r.pattern)?,
                replacement: r.replacement,
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
        out = rule
            .regex
            .replace_all(&out, rule.replacement.as_str())
            .into_owned();
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
            t("key sk-ant-api03-abcdefgh", "key [redacted]"),
            t("slack xoxb-1234567890-abcdef", "slack [redacted]"),
            t("aws AKIAABCDEFGHIJKLMNOP", "aws [redacted]"),
            t("g AIzaabcdefghijklmnopqrstuvwxyz012345678", "g [redacted]"),
            t(&format!("jwt {JWT}"), "jwt [redacted]"),
            t(PEM_IN, "[redacted]"),
            t("join beepjoin2abcdefghijklmnop", "join [redacted]"),
            t("join beepjoinabcdefghijklmnop", "join [redacted]"),
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
    fn redact_is_idempotent() {
        for (input, _) in table() {
            let once = redact(&input).expect("rules build");
            assert_eq!(redact(&once).expect("rules build"), once, "{input}");
        }
    }
}
