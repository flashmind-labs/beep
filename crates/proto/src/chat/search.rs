//! Finding a search in a chat's messages, the same way on the server and
//! in a client's local copy.

use std::ops::Range;

/// Characters a snippet keeps before the match.
const SNIPPET_BEFORE: usize = 40;
/// Characters a snippet keeps from the start of the match.
const SNIPPET_AFTER: usize = 100;
/// U+212A, which lowercases to `k`.
const KELVIN: char = '\u{212A}';

/// The words around the first place `query` appears in `text`, ignoring
/// case, or `None` when it does not.
pub fn match_snippet(text: &str, query: &str) -> Option<String> {
    find_ignoring_case(text, query).map(|at| snippet(text, at))
}

/// Where `query` first appears in `text`, ignoring case.
fn find_ignoring_case(text: &str, query: &str) -> Option<Range<usize>> {
    // Comparing char by char lowercases every character of every message,
    // which is slow over a long history. For a plain ASCII query the bytes
    // can be compared instead: the only other character whose lowercase is
    // ASCII is the Kelvin sign, so text that has one takes the slow way.
    if !query.is_empty() && query.is_ascii() && !text.contains(KELVIN) {
        return text
            .as_bytes()
            .windows(query.len())
            .position(|w| w.eq_ignore_ascii_case(query.as_bytes()))
            .map(|at| at..at + query.len());
    }
    text.char_indices().find_map(|(start, _)| {
        let mut rest = text[start..].char_indices();
        for wanted in query.chars() {
            let (_, got) = rest.next()?;
            if !got.to_lowercase().eq(wanted.to_lowercase()) {
                return None;
            }
        }
        let end = rest.next().map_or(text.len(), |(at, _)| start + at);
        Some(start..end)
    })
}

/// The words around `at` on one line, without Markdown marks, cut short
/// with `...` on either side.
fn snippet(text: &str, at: Range<usize>) -> String {
    let before: Vec<char> = text[..at.start].chars().collect();
    let from = before.len().saturating_sub(SNIPPET_BEFORE);
    let after: String = text[at.start..].chars().take(SNIPPET_AFTER).collect();
    let cut_after = text[at.start..].chars().count() > SNIPPET_AFTER;
    let mut snippet = String::new();
    if from > 0 {
        snippet.push_str("...");
    }
    let kept: String = before[from..].iter().collect();
    snippet.push_str(if from > 0 { kept.trim_start() } else { &kept });
    snippet.push_str(&after);
    let words: Vec<&str> = snippet.split_whitespace().collect();
    let mut snippet = words.join(" ").replace(['*', '`', '#'], "");
    if cut_after {
        snippet.push_str("...");
    }
    snippet
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippets_are_cut_around_the_match() {
        let text = format!("{} Kimpton {}", "a ".repeat(50), "b ".repeat(80));
        let at = find_ignoring_case(&text, "kimpton").unwrap();
        assert_eq!(&text[at.clone()], "Kimpton");
        let snippet = snippet(&text, at);
        assert!(snippet.starts_with("...a a"));
        assert!(snippet.ends_with("b b..."));
        assert!(snippet.contains("Kimpton"));
        assert_eq!(find_ignoring_case("ÉCOLE", "école"), Some(0..6));
        assert_eq!(match_snippet("no match", "boiler"), None);
    }

    #[test]
    fn ascii_queries_match_like_any_other() {
        assert_eq!(
            find_ignoring_case("Deploy the SERVER", "server"),
            Some(11..17)
        );
        assert_eq!(find_ignoring_case("naïve Server", "server"), Some(7..13));
        assert_eq!(find_ignoring_case("short", "longer than it"), None);
        // The Kelvin sign lowercases to k, so it still matches.
        assert_eq!(
            find_ignoring_case("10 \u{212A}elvin", "kelvin"),
            Some(3..11)
        );
    }
}
