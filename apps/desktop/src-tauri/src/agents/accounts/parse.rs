/// What every Claude Code OAuth token starts with.
const TOKEN_PREFIX: &str = "sk-ant-oat";

/// The longest line of `claude`'s passed on as the reason it failed.
const REASON_CHARS: usize = 200;

/// Why `claude setup-token` ended without a token: the last line it
/// drew.
pub(super) fn ended_without_token(output: &str) -> String {
    let text = plain(output);
    let last = text
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or_default();
    if last.is_empty() {
        return "Claude Code ended without a token. Paste one instead.".into();
    }

    let short: String = last.chars().take(REASON_CHARS).collect();
    format!("Claude Code ended without a token: {short}")
}

/// `text` without terminal escapes. A cursor move forward stands for
/// the spaces it skips; other moves end the line.
pub(super) fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.next() {
                Some('[') => {
                    let mut last = ' ';

                    for c in chars.by_ref() {
                        if c.is_ascii_alphabetic() || c == '~' {
                            last = c;
                            break;
                        }
                    }

                    match last {
                        'C' => out.push(' '),
                        'A' | 'B' | 'E' | 'F' | 'H' | 'f' => out.push('\n'),
                        _ => {}
                    }
                }
                // OSC, such as a hyperlink: up to BEL or ESC \.
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

fn token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// The OAuth token in `output`, once all of it is there: the line
/// after it must have come, as a token wrapped over several lines
/// goes on in the next.
pub(super) fn token(output: &str) -> Option<String> {
    let text = plain(output);
    let at = text.find(TOKEN_PREFIX)?;
    let mut lines = text[at..].lines();
    let mut token: String = lines
        .next()?
        .chars()
        .take_while(|c| token_char(*c))
        .collect();
    let mut ended = false;

    for line in lines {
        let line = line.trim();

        if !line.is_empty() && line.chars().all(token_char) {
            token.push_str(line);
            continue;
        }
        ended = true;
        break;
    }
    (ended && token.len() > TOKEN_PREFIX.len() + 20).then_some(token)
}

#[cfg(test)]
#[path = "../../tests/agents_accounts_parse.rs"]
mod tests;
