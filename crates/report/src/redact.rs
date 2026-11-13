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

struct Compiled {
    regex: Regex,
    replacement: String,
    skip: Skip,
}

type Built = Result<Compiled, regex::Error>;

fn rule(pattern: impl AsRef<str>, replacement: impl Into<String>, skip: Skip) -> Built {
    Ok(Compiled {
        regex: Regex::new(pattern.as_ref())?,
        replacement: replacement.into(),
        skip,
    })
}

fn whole(pattern: &str) -> Built {
    rule(pattern, MASK, Skip::Never)
}

fn keep(pattern: impl AsRef<str>) -> Built {
    rule(pattern, format!("${{1}}{MASK}"), Skip::Never)
}

fn rules() -> Result<Vec<Compiled>, regex::Error> {
    [
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
    .into_iter()
    .collect()
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

/// A key file's path, as in `ssh_key=/path/to/key`. Not when the name says secret some other
/// way, and not a value that could be base64 (which starts with `/` one time in 64).
fn key_path(key: &str, val: &str) -> bool {
    const SECRET: [&str; 8] = [
        "token",
        "secret",
        "passw",
        "api",
        "private",
        "access",
        "credential",
        "cookie",
    ];
    let name = key.trim_end_matches("id").trim_end_matches(['_', '-']);
    name.ends_with("key")
        && !SECRET.iter().any(|s| key.contains(s))
        && val.starts_with('/')
        && val[1..].contains('/')
        && !val.contains(['+', '='])
}

/// A null or boolean, or a short plain word: not a secret.
fn ordinary_word(val: &str) -> bool {
    let lower = val.to_ascii_lowercase();
    matches!(lower.as_str(), "null" | "none" | "nil" | "true" | "false")
        || (val.len() < 8 && val.chars().all(|c| c.is_ascii_alphabetic()))
}

static COMPILED: LazyLock<Result<Vec<Compiled>, regex::Error>> = LazyLock::new(rules);

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
#[path = "tests/redact.rs"]
mod tests;
