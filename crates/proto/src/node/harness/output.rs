//! Turning what an install or update command printed into text a person
//! can read: terminal escapes, progress bars and spinners taken out, and
//! the lines that say why it failed picked from the end.

const ESC: char = '\u{1b}';
const BEL: char = '\u{7}';
/// The one-character C1 forms of CSI and OSC.
const CSI: char = '\u{9b}';
const OSC: char = '\u{9d}';
/// Left where an escape was taken out, so a line that held only escapes
/// is told from a blank one. Never in the text otherwise: control
/// characters are dropped.
const MARK: char = '\u{1}';

/// `text` as a person reads it: ANSI and OSC escapes removed, each line
/// as a terminal leaves it after carriage returns overwrote it, progress
/// glyphs dropped, lines left with no words dropped, repeats and runs of
/// blank lines folded into one.
pub fn clean_output(text: &str) -> String {
    let plain = strip_escapes(skip_cut_escape(text));
    let mut lines: Vec<String> = Vec::new();
    for raw in plain.split('\n') {
        let escaped = raw.contains(MARK);
        let raw = raw.replace(MARK, "");
        let line = match last_overwrite(&raw) {
            Some(line) => line,
            None => {
                // A blank line stays; one that held only escapes goes.
                if !escaped {
                    lines.push(String::new());
                }
                continue;
            }
        };
        let had_glyphs = line.chars().any(is_progress_glyph);
        let line: String = line.chars().filter(|c| !is_progress_glyph(*c)).collect();
        let line = if had_glyphs {
            line.split_whitespace().collect::<Vec<_>>().join(" ")
        } else {
            line.trim_end().to_owned()
        };
        if !line.chars().any(char::is_alphabetic) {
            // Only bars, glyphs or numbers.
            continue;
        }
        if lines.last() == Some(&line) {
            continue;
        }
        lines.push(line);
    }
    let mut out = String::new();
    let mut blank = false;
    for line in lines {
        if line.is_empty() {
            blank = !out.is_empty();
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank { "\n\n" } else { "\n" });
        }
        blank = false;
        out.push_str(&line);
    }
    out
}

/// The lines of `clean` (as `clean_output` gives it) that say why the
/// command failed, on one line of at most about `max` characters: from
/// the earliest error line that still fits to the end, or else the start
/// of the last error line, or else the last words.
pub fn failure_line(clean: &str, max: usize) -> String {
    let lines: Vec<String> = clean
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect();
    let joined = |from: usize| lines[from..].join(" ");
    let errors: Vec<usize> = (0..lines.len())
        .filter(|&i| mentions_error(&lines[i]))
        .collect();
    if let Some(&last) = errors.last() {
        if let Some(&from) = errors
            .iter()
            .find(|&&from| joined(from).chars().count() <= max)
        {
            return joined(from);
        }
        return head(&joined(last), max);
    }
    tail(&joined(0), max)
}

/// Whether `line` reads as an error.
fn mentions_error(line: &str) -> bool {
    let lower = line.to_lowercase();
    ["error", "err!", "fatal", "failed", "denied", "not found"]
        .iter()
        .any(|word| lower.contains(word))
}

/// The first `max` characters of `text`, cut at a space.
fn head(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let cut: String = text.chars().take(max).collect();
    let cut = match cut.rfind(' ') {
        Some(space) if space > max / 2 => &cut[..space],
        _ => &cut,
    };
    format!("{}…", cut.trim_end())
}

/// The last `max` characters of `text`, cut at a space.
fn tail(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_owned();
    }
    let cut: String = text.chars().skip(count - max).collect();
    let cut = match cut.find(' ') {
        Some(space) if space < max / 2 => &cut[space + 1..],
        _ => &cut,
    };
    format!("…{}", cut.trim_start())
}

/// `text` without the rest of an SGR escape whose start was cut off, such
/// as the `238m` left when output is cut to its last characters.
fn skip_cut_escape(text: &str) -> &str {
    let digits = text
        .find(|c: char| !(c.is_ascii_digit() || c == ';'))
        .unwrap_or(text.len());
    if digits > 0 && text[digits..].starts_with('m') {
        &text[digits + 1..]
    } else {
        text
    }
}

/// `text` without its ANSI escapes (CSI, OSC, DCS and the short ones) and
/// control characters other than newline, tab and carriage return.
fn strip_escapes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ESC => match chars.next() {
                Some('[') => {
                    skip_csi(&mut chars);
                    out.push(MARK);
                }
                Some(']' | 'P' | 'X' | '^' | '_') => {
                    skip_string(&mut chars);
                    out.push(MARK);
                }
                // Charset and other short escapes: intermediates, then one
                // final character.
                Some(' '..='/') => {
                    while chars.next_if(|c| (' '..='/').contains(c)).is_some() {}
                    chars.next();
                    out.push(MARK);
                }
                _ => out.push(MARK),
            },
            CSI => {
                skip_csi(&mut chars);
                out.push(MARK);
            }
            OSC => {
                skip_string(&mut chars);
                out.push(MARK);
            }
            '\n' | '\t' | '\r' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Past a CSI sequence's parameters, intermediates and final byte.
fn skip_csi(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while chars.next_if(|c| ('0'..='?').contains(c)).is_some() {}
    while chars.next_if(|c| (' '..='/').contains(c)).is_some() {}
    chars.next_if(|c| ('@'..='~').contains(c));
}

/// Past an OSC (or DCS) string, which BEL or ESC `\` ends.
fn skip_string(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while let Some(c) = chars.next() {
        match c {
            BEL => return,
            ESC => {
                chars.next_if_eq(&'\\');
                return;
            }
            _ => {}
        }
    }
}

/// What a terminal shows of `line` after its carriage returns: the last
/// part with something in it. `None` when it is blank.
fn last_overwrite(line: &str) -> Option<&str> {
    line.split('\r').rev().find(|part| !part.trim().is_empty())
}

/// Block elements, shades and braille spinners: progress bars and logos.
fn is_progress_glyph(c: char) -> bool {
    matches!(c, '\u{2580}'..='\u{259f}' | '\u{2800}'..='\u{28ff}' | '\u{25a0}'..='\u{25ff}')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The end of what a failed `opencode upgrade` printed, cut to its last
    /// characters as an older node did: the tail of its logo in colored
    /// blocks, then its error.
    const OPENCODE: &str = "238m \u{1b}[0m\u{1b}[0m\u{2588}\u{1b}[0m\u{1b}[0m\u{2588}\u{1b}[0m\u{1b}[0m\u{1b}[48;5;238m\u{2580}\u{1b}[0m\u{1b}[0m\u{1b}[48;5;238m\u{2580}\u{1b}[0m\u{1b}[0m\u{1b}[48;5;238m\u{2580}\u{1b}[0m \u{1b}[90m\u{2580}\u{1b}[0m\u{1b}[90m\u{2580}\u{1b}[0m\u{1b}[90m\u{2580}\u{1b}[0m\u{1b}[90m\u{2580}\u{1b}[0m \u{1b}[90m\u{2588}\u{1b}[0m\u{1b}[90m\u{2580}\u{1b}[0m\u{1b}[38;5;235m\u{2580}\u{1b}[0m\u{1b}[38;5;235m\u{2580}\u{1b}[0m\u{1b}[90m\u{2580}\u{1b}[0m \u{1b}[0m\u{2580}\u{1b}[0m\u{1b}[0m\u{2580}\u{1b}[0m\u{1b}[0m\u{2580}\u{1b}[0m \u{1b}[0m \u{1b}[91m\u{1b}[1mError: \u{1b}[0mUnexpected error, check log file at /path/to/home/.local/share/opencode/log/2026-11-03T112330.log for more details SyntaxError: JSON Parse error: Unexpected EOF";

    #[test]
    fn the_opencode_sample_comes_down_to_its_error() {
        let clean = clean_output(OPENCODE);
        let expected = "Error: Unexpected error, check log file at /path/to/home/.local/share/opencode/log/2026-11-03T112330.log for more details SyntaxError: JSON Parse error: Unexpected EOF";
        assert_eq!(clean, expected);
        assert_eq!(failure_line(&clean, 300), expected);
    }

    #[test]
    fn the_same_error_on_its_own_lines_keeps_both() {
        let text = "\u{1b}[48;5;238m\u{2580}\u{2580}\u{2580}\u{1b}[0m\n\u{1b}[90m\u{2588}\u{2588}\u{1b}[0m\n\n\u{1b}[91m\u{1b}[1mError: \u{1b}[0mUnexpected error, check log file\nSyntaxError: JSON Parse error: Unexpected EOF\n";
        let clean = clean_output(text);
        assert_eq!(
            clean,
            "Error: Unexpected error, check log file\nSyntaxError: JSON Parse error: Unexpected EOF"
        );
        assert_eq!(
            failure_line(&clean, 300),
            "Error: Unexpected error, check log file SyntaxError: JSON Parse error: Unexpected EOF"
        );
    }

    #[test]
    fn progress_bars_and_spinners_are_dropped() {
        let text = "Downloading\r[####      ] 40%\r[##########] 100%\n\
            \u{280b} Resolving\r\u{2819} Resolving\r\u{2839} Resolving\n\
            \u{2593}\u{2593}\u{2593}\u{2591}\u{2591} 60%\n\
            \u{1b}]0;npm install\u{7}\u{1b}[2K\u{1b}[1G\n\
            \u{1b}[?25l\u{1b}[?25h\n\
            added 3 packages in 2s\n\
            added 3 packages in 2s\n";
        assert_eq!(clean_output(text), "Resolving\nadded 3 packages in 2s");
    }

    #[test]
    fn carriage_returns_leave_the_last_text_written() {
        assert_eq!(clean_output("one\rtwo\r\n"), "two");
        assert_eq!(
            clean_output("fetching 10%\rfetching done\r\x1b[K"),
            "fetching done"
        );
    }

    #[test]
    fn blank_lines_fold_and_text_is_kept() {
        let text = "\n\nfirst\n\n\n\nsecond   spaced\n\n";
        assert_eq!(clean_output(text), "first\n\nsecond   spaced");
    }

    #[test]
    fn the_failure_line_is_cut_to_length() {
        let long = format!("Error: {}", "word ".repeat(100));
        let line = failure_line(&long, 60);
        assert!(line.starts_with("Error: word"), "{line}");
        assert!(line.ends_with('…'), "{line}");
        assert!(line.chars().count() <= 61, "{line}");
        // Without an error line, the last words are kept.
        let words = (0..100)
            .map(|i| format!("w{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let line = failure_line(&words, 30);
        assert!(line.starts_with('…') && line.ends_with("w99"), "{line}");
        // The earliest error line that fits starts the line.
        let text = "Downloading formula\nError: refusing to load\nRun brew trust to trust it.";
        assert_eq!(
            failure_line(text, 300),
            "Error: refusing to load Run brew trust to trust it."
        );
    }
}
