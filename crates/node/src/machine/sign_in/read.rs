use beep_proto::node::sign_in::{State, Tool};

/// The longest line of the agent's passed on as the reason it failed.
const REASON_CHARS: usize = 200;

/// Where each agent sends the user: only these links are passed on.
fn hosts(tool: Tool) -> &'static [&'static str] {
    match tool {
        Tool::Claude => &["claude.com", "claude.ai", "anthropic.com"],
        Tool::Codex | Tool::Opencode => &["openai.com", "chatgpt.com"],
        Tool::Agy => &["accounts.google.com"],
    }
}

/// The link (and code) in what the agent printed so far, once the
/// lines that hold them are complete.
pub(super) fn prompt(tool: Tool, output: &str) -> Option<State> {
    let text = plain(output);
    // Only whole lines: the rest may still be on its way.
    let done = &text[..text.rfind('\n')?];
    let words = || done.split_whitespace();
    let url = words().find_map(|word| link(word, hosts(tool)))?;

    match tool {
        Tool::Claude | Tool::Agy => Some(State::Waiting {
            url,
            code: None,
            asks_code: true,
        }),
        Tool::Codex | Tool::Opencode => {
            let code = words()
                .map(|word| word.trim_end_matches(['.', ',']))
                .find(|word| is_code(word))?;
            Some(State::Waiting {
                url,
                code: Some(code.to_owned()),
                asks_code: false,
            })
        }
    }
}

/// `word` when it is an https link to one of `hosts` or under them.
pub(super) fn link(word: &str, hosts: &[&str]) -> Option<String> {
    let word = word.trim_matches(['.', ',', ')', '(', '"', '\'', '<', '>']);
    let rest = word.strip_prefix("https://")?;
    let host = rest
        .split(['/', '?', '#', ':'])
        .next()?
        .to_ascii_lowercase();
    let theirs = hosts
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")));
    let clean = word.chars().all(|c| c.is_ascii_graphic());
    (theirs && clean).then(|| word.to_owned())
}

/// A one-time code such as `ABCD-12345`: hyphen-joined capitals and digits.
pub(super) fn is_code(word: &str) -> bool {
    let groups: Vec<&str> = word.split('-').collect();
    (4..=20).contains(&word.len())
        && groups.len() >= 2
        && groups.iter().all(|group| {
            !group.is_empty()
                && group
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        })
}

/// What Claude Code asks with, on the line its answer then goes on.
const CLAUDE_ASKS: &str = "Paste code here if prompted >";

/// Why the sign-in ended: the last line the agent printed in words.
pub(super) fn failure(name: &str, output: &str) -> String {
    let text = plain(output);
    let last = text
        .lines()
        .map(|line| {
            let line = line.trim();
            line.strip_prefix(CLAUDE_ASKS).map_or(line, str::trim)
        })
        // Not Antigravity's JSON events either: its reason is the line before.
        .rfind(|line| !line.is_empty() && !line.starts_with("WARNING") && !line.starts_with('{'))
        .unwrap_or_default();
    if last.is_empty() {
        return format!("{name} could not sign in.");
    }

    let short: String = last.chars().take(REASON_CHARS).collect();
    format!("{name} could not sign in: {short}")
}

/// `text` without terminal escapes: colours and moves (CSI), and
/// hyperlinks and titles (OSC) with what they carry.
pub(super) fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.next() {
                Some('[') => {
                    for c in chars.by_ref() {
                        if c.is_ascii_alphabetic() || c == '~' {
                            // A move to a column stands for a space.
                            if matches!(c, 'C' | 'G') {
                                out.push(' ');
                            }
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(c) = chars.next() {
                        if c == '\u{7}' {
                            break;
                        }

                        if c == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\r' => {}
            '\n' | '\t' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }

    out
}

#[cfg(test)]
#[path = "../../tests/machine_sign_in_read.rs"]
mod tests;
